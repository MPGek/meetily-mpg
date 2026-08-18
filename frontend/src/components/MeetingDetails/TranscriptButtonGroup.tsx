"use client";

import { useState, useCallback, useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import { Copy, FolderOpen, RefreshCw, Users, Check } from 'lucide-react';
import Analytics from '@/lib/analytics';
import { RetranscribeDialog } from './RetranscribeDialog';
import { useConfig } from '@/contexts/ConfigContext';
import { recordingService } from '@/services/recordingService';
import { loadDiarizationSettings } from '@/lib/diarization';
import { useRouter } from 'next/navigation';
import { toast } from 'sonner';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';


interface TranscriptButtonGroupProps {
  transcriptCount: number;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => Promise<void>;
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
}


export function TranscriptButtonGroup({
  transcriptCount,
  onCopyTranscript,
  onOpenMeetingFolder,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
}: TranscriptButtonGroupProps) {
  const { betaFeatures } = useConfig();
  const router = useRouter();
  const [showRetranscribeDialog, setShowRetranscribeDialog] = useState(false);
  const [isDiarizing, setIsDiarizing] = useState(false);
  const [modelsReady, setModelsReady] = useState(false);

  useEffect(() => {
    let cancelled = false;
    recordingService.checkDiarizationModels()
      .then((status) => {
        if (!cancelled) {
          setModelsReady(status.segmentation_ready && status.embedding_ready);
        }
      })
      .catch((error) => {
        console.error('Failed to check diarization models:', error);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleRetranscribeComplete = useCallback(async () => {
    if (onRefetchTranscripts) {
      await onRefetchTranscripts();
    }
  }, [onRefetchTranscripts]);

  const openSettings = () => {
    router.push('/settings?tab=general');
  };

  const handleReanalyzeSpeakers = async () => {
    if (!meetingId || isDiarizing) return;
    setIsDiarizing(true);
    try {
      Analytics.trackButtonClick('reanalyze_speakers', 'meeting_details');
      const settings = loadDiarizationSettings();
      const maxSpeakers = settings.maxSpeakers > 0 ? settings.maxSpeakers : undefined;
      await recordingService.startDiarization(
        meetingId,
        maxSpeakers
      );
      // Refetch handled by useDiarizationProgress onComplete event
    } catch (err: any) {
      console.error('Diarization failed:', err);
      const message = err instanceof Error ? err.message : String(err);
      const isModelMissing = /model.*not found|download models/i.test(message);
      toast.error('Speaker analysis failed', {
        description: message,
        action: isModelMissing
          ? {
              label: 'Open Settings',
              onClick: openSettings,
            }
          : undefined,
      });
    } finally {
      setIsDiarizing(false);
    }
  };

  return (
    <div className="flex items-center justify-center w-full gap-2">
      <ButtonGroup>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            Analytics.trackButtonClick('copy_transcript', 'meeting_details');
            onCopyTranscript();
          }}
          disabled={transcriptCount === 0}
          title={transcriptCount === 0 ? 'No transcript available' : 'Copy Transcript'}
        >
          <Copy />
          <span className="hidden lg:inline">Copy</span>
        </Button>

        <Button
          size="sm"
          variant="outline"
          className="xl:px-4"
          onClick={() => {
            Analytics.trackButtonClick('open_recording_folder', 'meeting_details');
            onOpenMeetingFolder();
          }}
          title="Open Recording Folder"
        >
          <FolderOpen className="xl:mr-2" size={18} />
          <span className="hidden lg:inline">Recording</span>
        </Button>

        {betaFeatures.importAndRetranscribe && meetingId && meetingFolderPath && (
          <Button
            size="sm"
            variant="outline"
            className="bg-gradient-to-r from-blue-50 to-purple-50 hover:from-blue-100 hover:to-purple-100 border-blue-200 xl:px-4"
            onClick={() => {
              Analytics.trackButtonClick('enhance_transcript', 'meeting_details');
              setShowRetranscribeDialog(true);
            }}
            title="Retranscribe to enhance your recorded audio"
          >
            <RefreshCw className="xl:mr-2" size={18} />
            <span className="hidden lg:inline">Enhance</span>
          </Button>
        )}

        {meetingId && modelsReady && (
          <Button
            size="sm"
            variant="outline"
            onClick={handleReanalyzeSpeakers}
            disabled={isDiarizing}
            title="Identify who spoke when"
          >
            {isDiarizing ? (
              <RefreshCw className="xl:mr-2 animate-spin" size={18} />
            ) : (
              <Users className="xl:mr-2" size={18} />
            )}
            <span className="hidden lg:inline">{isDiarizing ? 'Analyzing...' : 'Speakers'}</span>
          </Button>
        )}

        {meetingId && !modelsReady && (
          <Button
            size="sm"
            variant="outline"
            onClick={openSettings}
            title="Download diarization models in settings"
          >
            <Users className="xl:mr-2" size={18} />
            <span className="hidden lg:inline">Setup Speakers</span>
          </Button>
        )}
      </ButtonGroup>

      {meetingId && modelsReady && (
        <ExpectedSpeakersSelector
          meetingId={meetingId}
          onRematch={onRefetchTranscripts}
        />
      )}

      {betaFeatures.importAndRetranscribe && meetingId && meetingFolderPath && (
        <RetranscribeDialog
          open={showRetranscribeDialog}
          onOpenChange={setShowRetranscribeDialog}
          meetingId={meetingId}
          meetingFolderPath={meetingFolderPath}
          onComplete={handleRetranscribeComplete}
        />
      )}
    </div>
  );
}

// Multi-select popover for choosing expected speakers per meeting.
// When the selection changes, saves to DB and optionally triggers re-match.
function ExpectedSpeakersSelector({
  meetingId,
  onRematch,
}: {
  meetingId: string;
  onRematch?: () => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const [allSpeakers, setAllSpeakers] = useState<Array<{ id: string; name: string }>>([]);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(false);

  // Load registry speakers and current expected-speaker list when popover opens
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setLoading(true);
    Promise.all([
      recordingService.listSpeakers(),
      recordingService.getExpectedSpeakers(meetingId),
    ]).then(([speakers, expected]) => {
      if (!cancelled) {
        setAllSpeakers(speakers);
        setSelectedIds(new Set(expected));
        setLoading(false);
      }
    }).catch(() => {
      if (!cancelled) setLoading(false);
    });
    return () => { cancelled = true; };
  }, [open, meetingId]);

  const toggleSpeaker = async (speakerId: string) => {
    const next = new Set(selectedIds);
    if (next.has(speakerId)) {
      next.delete(speakerId);
    } else {
      next.add(speakerId);
    }
    setSelectedIds(next);
    try {
      await recordingService.setExpectedSpeakers(meetingId, Array.from(next));
      // Re-match from cached centroids (no audio re-processing)
      await recordingService.rematchMeetingSpeakers(meetingId);
      toast.success('Expected speakers updated', { duration: 2000 });
      if (onRematch) await onRematch();
    } catch (error) {
      console.error('Failed to update expected speakers:', error);
      toast.error('Failed to update expected speakers');
    }
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          size="sm"
          variant="outline"
          title="Select expected speakers for auto-recognition"
        >
          <Users className="xl:mr-2" size={18} />
          <span className="hidden lg:inline">
            {selectedIds.size > 0 ? `${selectedIds.size} Expected` : 'Expected'}
          </span>
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-56 p-0" align="start">
        <div className="px-3 py-2 border-b">
          <span className="text-xs font-medium text-gray-700">Expected Speakers</span>
          <p className="text-[10px] text-gray-400 mt-0.5">
            Auto-recognition matches only these. Empty = match all.
          </p>
        </div>
        <div className="max-h-48 overflow-y-auto">
          {loading ? (
            <div className="px-3 py-2 text-xs text-gray-400">Loading...</div>
          ) : allSpeakers.length === 0 ? (
            <div className="px-3 py-2 text-xs text-gray-400">No speakers in registry</div>
          ) : (
            allSpeakers.map((sp) => (
              <button
                key={sp.id}
                className="w-full flex items-center gap-2 px-3 py-1.5 text-xs hover:bg-gray-100 text-left"
                onClick={() => toggleSpeaker(sp.id)}
              >
                <div className={`w-4 h-4 rounded border flex items-center justify-center ${
                  selectedIds.has(sp.id)
                    ? 'bg-blue-600 border-blue-600'
                    : 'border-gray-300'
                }`}>
                  {selectedIds.has(sp.id) && (
                    <Check className="h-3 w-3 text-white" />
                  )}
                </div>
                <span className="truncate">{sp.name}</span>
              </button>
            ))
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
