"use client";

import { Transcript, TranscriptSegmentData } from '@/types';
import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { TranscriptButtonGroup } from './TranscriptButtonGroup';
import { AudioPlayer, AudioPlayerHandle, PlaybackState } from '@/components/AudioPlayer';
import { useMemo, useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

interface TranscriptPanelProps {
  transcripts: Transcript[];
  customPrompt: string;
  onPromptChange: (value: string) => void;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => Promise<void>;
  isRecording: boolean;
  disableAutoScroll?: boolean;

  // Optional pagination props (when using virtualization)
  usePagination?: boolean;
  segments?: TranscriptSegmentData[];
  hasMore?: boolean;
  isLoadingMore?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;

  // Retranscription props
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
  /** In-place speaker relabel (design D11): updates only the affected
   *  segment(s) in local paginated state — no refetch, no scroll reset. */
  onUpdateSpeakerLabelLocal?: (speaker: string, label: string, transcriptId?: string) => void;

  // Diarization progress
  diarizationProgress?: {
    status: string | null;
    progress: number;
    message: string;
    isProcessing: boolean;
  } | null;
}

export function TranscriptPanel({
  transcripts,
  customPrompt,
  onPromptChange,
  onCopyTranscript,
  onOpenMeetingFolder,
  isRecording,
  disableAutoScroll = false,
  usePagination = false,
  segments,
  hasMore,
  isLoadingMore,
  totalCount,
  loadedCount,
  onLoadMore,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
  onUpdateSpeakerLabelLocal,
  diarizationProgress,
}: TranscriptPanelProps) {
  const handleUpdateSpeakerLabel = useCallback(async (speaker: string, label: string, transcriptId?: string) => {
    // The registry binding/override is handled by the combobox depending on
    // its scope: single-block -> `assign_block_speaker` (per-transcript
    // override), apply-to-all -> `assign_speaker` (cluster link + enroll).
    // Here we only propagate the new name into local state, in place: no
    // refetch, so scroll position and the rest of the list are untouched
    // (design D11). The backend write already succeeded before this runs.
    if (onUpdateSpeakerLabelLocal) {
      onUpdateSpeakerLabelLocal(speaker, label, transcriptId);
      return;
    }
    // Fallback (no local updater): refresh resolved display names from DB.
    try {
      if (onRefetchTranscripts) {
        await onRefetchTranscripts();
      }
    } catch (error) {
      console.error('Failed to refresh speaker labels:', error);
      toast.error('Failed to refresh speaker labels');
    }
  }, [onUpdateSpeakerLabelLocal, onRefetchTranscripts]);

  // Resolve the meeting's audio file path for playback
  const [audioPath, setAudioPath] = useState<string | null>(null);
  const [isAudioPlaying, setIsAudioPlaying] = useState(false);
  const [playerTime, setPlayerTime] = useState(0);
  const [isEnded, setIsEnded] = useState(false);
  const [hasStarted, setHasStarted] = useState(false);
  const audioPlayerRef = useRef<AudioPlayerHandle>(null);
  const prevActiveSegmentRef = useRef<string | null>(null);

  useEffect(() => {
    if (!meetingId) return;
    let cancelled = false;
    setHasStarted(false);
    setIsEnded(false);
    setPlayerTime(0);
    prevActiveSegmentRef.current = null;
    invoke<string | null>('get_meeting_audio_path', { meetingId })
      .then((path) => {
        if (!cancelled) setAudioPath(path);
      })
      .catch((error) => {
        console.error('Failed to resolve meeting audio path:', error);
      });
    return () => {
      cancelled = true;
    };
  }, [meetingId]);

  const handlePlayFrom = useCallback((startTime: number) => {
    setHasStarted(true);
    setIsEnded(false);
    void audioPlayerRef.current?.playFrom(startTime);
  }, []);

  const handlePlaybackStateChange = useCallback((state: PlaybackState) => {
    setIsAudioPlaying(state.isPlaying);
    if (state.isPlaying) {
      setHasStarted(true);
      setIsEnded(false);
    }
    setIsEnded((prev) => prev || state.endedCount > 0);
  }, []);

  const handleTimeUpdate = useCallback((time: number) => {
    setPlayerTime(time);
  }, []);

  // Convert transcripts to segments if pagination is not used but we want virtualization
  const convertedSegments = useMemo(() => {
    if (usePagination && segments) {
      return segments;
    }
    // Convert transcripts to segments for virtualization
    return transcripts.map(t => ({
      id: t.id,
      timestamp: t.audio_start_time ?? 0,
      endTime: t.audio_end_time,
      hasAudioTime: t.audio_start_time !== undefined && t.audio_start_time !== null,
      text: t.text,
      confidence: t.confidence,
      source_device: t.source_device,
      speaker: t.speaker,
      speaker_label: t.speaker_label,
    }));
  }, [transcripts, usePagination, segments]);

  // Derive the segment containing the current playback position
  const activeSegmentId = useMemo(() => {
    if (!hasStarted || isEnded || convertedSegments.length === 0) return null;
    const segments = convertedSegments;
    let lo = 0;
    let hi = segments.length - 1;
    let idx = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (segments[mid].timestamp <= playerTime) {
        idx = mid;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
    if (idx === -1) return null;
    const seg = segments[idx];
    const end = seg.endTime ?? (idx + 1 < segments.length ? segments[idx + 1].timestamp : Number.POSITIVE_INFINITY);
    return playerTime < end ? seg.id : null;
  }, [hasStarted, isEnded, convertedSegments, playerTime]);

  // Auto-scroll to the active segment as playback moves
  useEffect(() => {
    if (!isAudioPlaying) return;
    if (!activeSegmentId || activeSegmentId === prevActiveSegmentRef.current) return;
    prevActiveSegmentRef.current = activeSegmentId;
    const el = document.getElementById(`segment-${activeSegmentId}`);
    el?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }, [activeSegmentId, isAudioPlaying]);

  return (
    <div className="hidden md:flex md:w-1/4 lg:w-1/3 min-w-0 border-r border-gray-200 bg-white flex-col relative shrink-0">
      {/* Title area */}
      <div className="p-4 border-b border-gray-200">
        <TranscriptButtonGroup
          transcriptCount={usePagination ? (totalCount ?? convertedSegments.length) : (transcripts?.length || 0)}
          onCopyTranscript={onCopyTranscript}
          onOpenMeetingFolder={onOpenMeetingFolder}
          meetingId={meetingId}
          meetingFolderPath={meetingFolderPath}
          onRefetchTranscripts={onRefetchTranscripts}
        />
      </div>

      {/* Audio player bar under the top buttons */}
      <AudioPlayer
        ref={audioPlayerRef}
        audioPath={audioPath}
        onPlaybackStateChange={handlePlaybackStateChange}
        onTimeUpdate={handleTimeUpdate}
      />

      {/* Diarization progress bar */}
      {diarizationProgress?.isProcessing && (
        <div className="px-4 py-2 border-b border-gray-200 bg-blue-50">
          <div className="flex items-center justify-between mb-1">
            <span className="text-xs font-medium text-blue-700">{diarizationProgress.message}</span>
            <span className="text-xs font-semibold text-blue-700">{Math.round(diarizationProgress.progress)}%</span>
          </div>
          <div className="w-full h-1.5 bg-blue-100 rounded-full overflow-hidden">
            <div
              className="h-full bg-blue-600 rounded-full transition-all duration-300"
              style={{ width: `${diarizationProgress.progress}%` }}
            />
          </div>
        </div>
      )}

      {/* Transcript content - use virtualized view for better performance */}
      <div className="flex-1 overflow-hidden pb-4">
        <VirtualizedTranscriptView
          segments={convertedSegments}
          isRecording={isRecording}
          isPaused={false}
          isProcessing={false}
          isStopping={false}
          enableStreaming={false}
          showConfidence={true}
          disableAutoScroll={disableAutoScroll}
          hasMore={hasMore}
          isLoadingMore={isLoadingMore}
          totalCount={totalCount}
          loadedCount={loadedCount}
          onLoadMore={onLoadMore}
          onUpdateSpeakerLabel={handleUpdateSpeakerLabel}
          meetingId={meetingId}
          onPlayFrom={audioPath ? handlePlayFrom : undefined}
          isAudioPlaying={isAudioPlaying}
          activeSegmentId={activeSegmentId}
        />
      </div>

      {/* Custom prompt input at bottom of transcript section */}
      {!isRecording && convertedSegments.length > 0 && (
        <div className="p-1 border-t border-gray-200">
          <textarea
            placeholder="Add context for AI summary. For example people involved, meeting overview, objective etc..."
            className="w-full px-3 py-2 border border-gray-200 rounded-md text-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 bg-white shadow-sm min-h-[80px] resize-y"
            value={customPrompt}
            onChange={(e) => onPromptChange(e.target.value)}
          />
        </div>
      )}
    </div>
  );
}
