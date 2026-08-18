'use client';

import { useCallback, useRef, useReducer, startTransition, useEffect, useState, memo, useMemo } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useAutoScroll } from "@/hooks/useAutoScroll";
import { useTranscriptStreaming } from "@/hooks/useTranscriptStreaming";
import { ConfidenceIndicator } from "./ConfidenceIndicator";
import { Tooltip, TooltipContent, TooltipTrigger } from "./ui/tooltip";
import { RecordingStatusBar } from "./RecordingStatusBar";
import { Popover, PopoverContent, PopoverTrigger } from "./ui/popover";
import { recordingService } from "@/services/recordingService";
import { toast } from "sonner";
import { Check } from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { Pause, Play } from "lucide-react";
import { TranscriptSegmentData } from "@/types";

export interface VirtualizedTranscriptViewProps {
    /** Transcript segments to display */
    segments: TranscriptSegmentData[];
    /** Whether recording is in progress */
    isRecording?: boolean;
    /** Whether recording is paused */
    isPaused?: boolean;
    /** Whether processing/finalizing transcription */
    isProcessing?: boolean;
    /** Whether stopping */
    isStopping?: boolean;
    /** Enable streaming effect for latest segment */
    enableStreaming?: boolean;
    /** Show confidence indicators */
    showConfidence?: boolean;
    /** Completely disable auto-scroll behavior (for meeting details page) */
    disableAutoScroll?: boolean;

    // Pagination props (infinite scroll)
    hasMore?: boolean;
    isLoadingMore?: boolean;
    totalCount?: number;
    loadedCount?: number;
    onLoadMore?: () => void;

    // Speaker label editing. Third arg is the transcript id for single-block
    // updates (used by the live view to relabel just that segment).
    onUpdateSpeakerLabel?: (speaker: string, label: string, transcriptId?: string) => Promise<void>;

    // Meeting ID for speaker registry operations
    meetingId?: string;

    // Audio playback from a segment's start time (meeting details page)
    onPlayFrom?: (startTime: number) => void;
    /** Whether the meeting audio player is currently playing */
    isAudioPlaying?: boolean;
    /** Id of the transcript segment currently being played */
    activeSegmentId?: string | null;
}

// Threshold for enabling virtualization (below this, use simple rendering)
const VIRTUALIZATION_THRESHOLD = 10;

// Helper function to format seconds as recording-relative time [MM:SS]
function formatRecordingTime(seconds: number | undefined): string {
    if (seconds === undefined) return '[--:--]';

    const totalSeconds = Math.floor(seconds);
    const minutes = Math.floor(totalSeconds / 60);
    const secs = totalSeconds % 60;

    return `[${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}]`;
}

// Speaker color palette — 8 distinct colors
const SPEAKER_COLORS = [
    "#3B82F6", // blue
    "#10B981", // emerald
    "#F59E0B", // amber
    "#8B5CF6", // violet
    "#F43F5E", // rose
    "#06B6D4", // cyan
    "#F97316", // orange
    "#14B8A6", // teal
];

// Stable color for IDs without a numeric index (e.g. legacy "SystemAudio")
const FALLBACK_SPEAKER_COLOR = "#6B7280"; // gray

function getSpeakerColor(speaker: string): string {
    const idx = parseInt(speaker.replace("MIC_SPEAKER_", "").replace("SPEAKER_", ""), 10);
    if (isNaN(idx)) return FALLBACK_SPEAKER_COLOR;
    return SPEAKER_COLORS[idx % SPEAKER_COLORS.length];
}

function formatSpeakerId(speaker: string): string {
    if (speaker === "SystemAudio") return "System Audio";
    if (speaker.startsWith("MIC_SPEAKER_")) {
        const idx = parseInt(speaker.replace("MIC_SPEAKER_", ""), 10);
        return `Mic Speaker ${idx + 1}`;
    }
    const idx = parseInt(speaker.replace("SPEAKER_", ""), 10);
    return isNaN(idx) ? speaker : `Speaker ${idx + 1}`;
}

// Inline editable speaker label — combobox with registry dropdown + free text.
// Default scope is "this block" (per-transcript override); an explicit
// "apply to all blocks of this speaker" option links the whole cluster.
function SpeakerLabel({
    speaker,
    label,
    color,
    onUpdate,
    meetingId,
    transcriptId,
    startTime,
    endTime,
}: {
    speaker: string;
    label?: string;
    color: string;
    onUpdate?: (speaker: string, label: string, transcriptId?: string) => Promise<void>;
    meetingId?: string;
    transcriptId?: string;
    startTime?: number;
    endTime?: number;
}) {
    const [open, setOpen] = useState(false);
    const [query, setQuery] = useState("");
    const [registrySpeakers, setRegistrySpeakers] = useState<Array<{ id: string; name: string }>>([]);
    const [loading, setLoading] = useState(false);
    const [scopeAll, setScopeAll] = useState(false);

    const displayName = label || formatSpeakerId(speaker);

    // Load registry speakers when popover opens
    useEffect(() => {
        if (!open) return;
        let cancelled = false;
        setLoading(true);
        recordingService.listSpeakers().then((speakers) => {
            if (!cancelled) {
                setRegistrySpeakers(speakers);
                setLoading(false);
            }
        }).catch(() => {
            if (!cancelled) setLoading(false);
        });
        return () => { cancelled = true; };
    }, [open]);

    const filtered = useMemo(() => {
        if (!query.trim()) return registrySpeakers;
        const q = query.toLowerCase();
        return registrySpeakers.filter((s) => s.name.toLowerCase().includes(q));
    }, [registrySpeakers, query]);

    const handleAssign = async (sp: { id: string; name: string }) => {
        setOpen(false);
        setQuery("");
        setScopeAll(false);
        if (!onUpdate) return;
        try {
            if (meetingId) {
                if (scopeAll) {
                    // Link the whole cluster (matched_by='user' + enrollment).
                    await recordingService.assignSpeaker(meetingId, speaker, sp.id);
                } else if (transcriptId) {
                    // Default: relabel only this block via a per-transcript override.
                    await recordingService.assignBlockSpeaker(transcriptId, sp.id);
                } else {
                    await recordingService.assignSpeaker(meetingId, speaker, sp.id);
                }
            } else {
                // Live recording view: cluster-wide via the prototype store,
                // or a per-turn override on this block when scoped.
                if (scopeAll || !transcriptId) {
                    await recordingService.assignLiveSpeaker(speaker, sp.id);
                } else {
                    await recordingService.assignLiveSpeakerBlock(
                        speaker,
                        startTime ?? 0,
                        sp.id,
                        undefined,
                        endTime
                    );
                }
            }
            // Scope-aware local propagation: apply-to-all passes no
            // transcriptId so the updater relabels every block of the
            // cluster; single-block passes the id (design D11).
            await onUpdate(speaker, sp.name, scopeAll ? undefined : transcriptId);
        } catch (error) {
            console.error("Failed to assign speaker:", error);
            // Backend write runs before onUpdate, so the local label was never
            // applied — the list is undisturbed. Surface the failure (9.3).
            toast.error("Failed to assign speaker");
        }
    };

    const handleCreateNew = async (name: string) => {
        const trimmed = name.trim();
        if (!trimmed) return;
        setOpen(false);
        setQuery("");
        setScopeAll(false);
        if (!onUpdate) return;
        try {
            if (meetingId) {
                if (scopeAll) {
                    await recordingService.assignSpeaker(meetingId, speaker, undefined, trimmed);
                } else if (transcriptId) {
                    await recordingService.assignBlockSpeaker(transcriptId, undefined, trimmed);
                } else {
                    await recordingService.assignSpeaker(meetingId, speaker, undefined, trimmed);
                }
            } else {
                if (scopeAll || !transcriptId) {
                    await recordingService.assignLiveSpeaker(speaker, undefined, trimmed);
                } else {
                    await recordingService.assignLiveSpeakerBlock(
                        speaker,
                        startTime ?? 0,
                        undefined,
                        trimmed,
                        endTime
                    );
                }
            }
            await onUpdate(speaker, trimmed, scopeAll ? undefined : transcriptId);
        } catch (error) {
            console.error("Failed to create speaker:", error);
            toast.error("Failed to assign speaker");
        }
    };

    const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
        if (e.key === "Enter") {
            e.preventDefault();
            const val = query.trim();
            if (val) {
                // Check if exact match exists
                const exact = registrySpeakers.find(
                    (s) => s.name.toLowerCase() === val.toLowerCase()
                );
                if (exact) {
                    handleAssign(exact);
                } else {
                    handleCreateNew(val);
                }
            }
        } else if (e.key === "Escape") {
            setOpen(false);
            setQuery("");
        }
    };

    if (!onUpdate) {
        return (
            <span
                className="text-xs font-medium text-gray-600"
                style={{ color }}
            >
                {displayName}
            </span>
        );
    }

    return (
        <Popover open={open} onOpenChange={setOpen}>
            <PopoverTrigger asChild>
                <span
                    className="text-xs font-medium text-gray-600 cursor-pointer hover:underline"
                    style={{ color }}
                    title="Click to rename speaker"
                >
                    {displayName}
                </span>
            </PopoverTrigger>
            <PopoverContent className="w-64 p-0" align="start">
                <div className="border-b px-3 py-2">
                    <input
                        autoFocus
                        placeholder="Search or type name..."
                        value={query}
                        onChange={(e) => setQuery(e.target.value)}
                        onKeyDown={handleKeyDown}
                        className="w-full text-xs bg-transparent outline-none placeholder:text-gray-400"
                    />
                </div>
                <div className="max-h-48 overflow-y-auto">
                    {loading ? (
                        <div className="px-3 py-2 text-xs text-gray-400">Loading...</div>
                    ) : filtered.length > 0 ? (
                        filtered.map((sp) => (
                            <button
                                key={sp.id}
                                className="w-full flex items-center gap-2 px-3 py-1.5 text-xs hover:bg-gray-100 text-left"
                                onClick={() => handleAssign(sp)}
                            >
                                <Check className="h-3 w-3 shrink-0 opacity-0" />
                                <span className="truncate">{sp.name}</span>
                            </button>
                        ))
                    ) : (
                        <div className="px-3 py-2 text-xs text-gray-400">
                            {query.trim()
                                ? `Press Enter to create "${query.trim()}"`
                                : "No speakers yet"}
                        </div>
                    )}
                </div>
                {query.trim() && !registrySpeakers.some(
                    (s) => s.name.toLowerCase() === query.trim().toLowerCase()
                ) && (
                    <div className="border-t px-3 py-2">
                        <button
                            className="w-full text-xs text-left text-blue-600 hover:underline"
                            onClick={() => handleCreateNew(query)}
                        >
                            Create "{query.trim()}"
                        </button>
                    </div>
                )}
                <div className="border-t px-3 py-2 flex items-center gap-2">
                    <label className="flex items-center gap-1.5 text-xs text-gray-600 cursor-pointer">
                        <input
                            type="checkbox"
                            checked={scopeAll}
                            onChange={(e) => setScopeAll(e.target.checked)}
                        />
                        Apply to all blocks of this speaker
                    </label>
                </div>
            </PopoverContent>
        </Popover>
    );
}

// Helper function to remove filler words and repetitions
function cleanStopWords(text: string): string {
    const stopWords = ['uh', 'um', 'er', 'ah', 'hmm', 'hm', 'eh', 'oh'];

    let cleanedText = text;
    stopWords.forEach(word => {
        const pattern = new RegExp(`\\b${word}\\b[,\\s]*`, 'gi');
        cleanedText = cleanedText.replace(pattern, ' ');
    });

    return cleanedText.replace(/\s+/g, ' ').trim();
}

// Memoized transcript segment component
const TranscriptSegment = memo(function TranscriptSegment({
    id,
    timestamp,
    endTime,
    text,
    confidence,
    isStreaming,
    showConfidence,
    source_device,
    speaker,
    speaker_label,
    hasAudioTime,
    onUpdateSpeakerLabel,
    meetingId,
    onPlayFrom,
    isActive,
    isAudioPlaying,
}: {
    id: string;
    timestamp: number;
    endTime?: number;
    text: string;
    confidence?: number;
    isStreaming: boolean;
    showConfidence: boolean;
    source_device?: string;
    speaker?: string;
    speaker_label?: string;
    hasAudioTime?: boolean;
    onUpdateSpeakerLabel?: (speaker: string, label: string, transcriptId?: string) => Promise<void>;
    meetingId?: string;
    onPlayFrom?: (startTime: number) => void;
    isActive?: boolean;
    isAudioPlaying?: boolean;
}) {
    const displayText = cleanStopWords(text) || (text.trim() === '' ? '[Silence]' : text);

    const isMic = source_device === 'Microphone';
    const isSystem = source_device === 'System';
    const isLegacy = !isMic && !isSystem;
    const hasSpeaker = !!speaker;

    const showPlayButton = !!onPlayFrom && !!hasAudioTime;
    const isActivePlaying = !!isActive && !!isAudioPlaying;

    const playButton = showPlayButton ? (
        <button
            type="button"
            onClick={(e) => {
                e.stopPropagation();
                onPlayFrom?.(timestamp);
            }}
            aria-label={`Play from ${formatRecordingTime(timestamp)}`}
            title={`Play from ${formatRecordingTime(timestamp)}`}
            className={`flex-shrink-0 w-5 h-5 flex items-center justify-center rounded-full mt-1 transition-colors ${
                isActivePlaying
                    ? 'text-blue-600 bg-blue-100'
                    : isActive
                    ? 'text-blue-500 bg-blue-50'
                    : 'text-gray-400 hover:text-blue-600 hover:bg-blue-50'
            }`}
        >
            {isActivePlaying ? <Pause className="w-3 h-3" /> : <Play className="w-3 h-3" />}
        </button>
    ) : null;

    const speakerColor = hasSpeaker ? getSpeakerColor(speaker) : undefined;

    if (isLegacy) {
        // Legacy neutral style - left-aligned, no bubble
        return (
            <div
                id={`segment-${id}`}
                className={isActive ? 'mb-3 bg-blue-50/70 rounded-lg ring-1 ring-blue-300' : 'mb-3'}
            >
                <div className="flex items-start gap-2">
                    <Tooltip>
                        <TooltipTrigger>
                            <span className="text-xs text-gray-400 mt-1 flex-shrink-0 min-w-[50px]">
                                {formatRecordingTime(timestamp)}
                            </span>
                        </TooltipTrigger>
                        <TooltipContent>
                            {confidence !== undefined && showConfidence && (
                                <ConfidenceIndicator confidence={confidence} showIndicator={showConfidence} />
                            )}
                        </TooltipContent>
                    </Tooltip>
                    {playButton}
                    <div className="flex-1">
                        {hasSpeaker && (
                            <div className="flex items-center gap-1.5 mb-1 ml-1">
                                <span
                                    className="inline-block w-2.5 h-2.5 rounded-full flex-shrink-0"
                                    style={{ backgroundColor: speakerColor }}
                                />
                                <SpeakerLabel
                                    speaker={speaker}
                                    label={speaker_label}
                                    color={speakerColor!}
                                    onUpdate={onUpdateSpeakerLabel}
                                    meetingId={meetingId}
                                    transcriptId={id}
                                    startTime={timestamp}
                                    endTime={endTime}
                                />
                            </div>
                        )}
                        {isStreaming ? (
                            <div
                                className="bg-gray-100 border border-gray-200 rounded-lg px-3 py-2"
                                style={hasSpeaker ? { borderLeftColor: speakerColor, borderLeftWidth: 3 } : undefined}
                            >
                                <p className="text-base text-gray-800 leading-relaxed">{displayText}</p>
                            </div>
                        ) : (
                            <p className="text-base text-gray-800 leading-relaxed">{displayText}</p>
                        )}
                    </div>
                </div>
            </div>
        );
    }

    if (isMic) {
        return (
            <div id={`segment-${id}`} className="mb-3">
                <div className="flex items-start gap-2">
                    <Tooltip>
                        <TooltipTrigger>
                            <span className="text-xs text-gray-400 mt-1 flex-shrink-0 min-w-[50px]">
                                {formatRecordingTime(timestamp)}
                            </span>
                        </TooltipTrigger>
                        <TooltipContent>
                            {confidence !== undefined && showConfidence && (
                                <ConfidenceIndicator confidence={confidence} showIndicator={showConfidence} />
                            )}
                        </TooltipContent>
                    </Tooltip>
                    {playButton}
                    <div className="flex-1 max-w-[80%]">
                        {hasSpeaker && (
                            <div className="flex items-center gap-1.5 mb-1 ml-1">
                                <span
                                    className="inline-block w-2.5 h-2.5 rounded-full flex-shrink-0"
                                    style={{ backgroundColor: speakerColor }}
                                />
                                <SpeakerLabel
                                    speaker={speaker}
                                    label={speaker_label}
                                    color={speakerColor!}
                                    onUpdate={onUpdateSpeakerLabel}
                                    meetingId={meetingId}
                                    transcriptId={id}
                                    startTime={timestamp}
                                    endTime={endTime}
                                />
                            </div>
                        )}
                        <div
                            className={`bg-blue-50 border border-blue-100 rounded-lg px-3 py-2 ${isActive ? 'ring-2 ring-blue-400' : ''}`}
                            style={hasSpeaker ? { borderLeftColor: speakerColor, borderLeftWidth: 3 } : undefined}
                        >
                            <p className="text-base text-gray-800 leading-relaxed">{displayText}</p>
                        </div>
                    </div>
                </div>
            </div>
        );
    }

    // System: right-aligned, timestamp on right, green bubble
    return (
        <div id={`segment-${id}`} className="mb-3">
            <div className="flex items-start gap-2 justify-end">
                <div className="flex-1 max-w-[80%]">
                    {hasSpeaker && (
                        <div className="flex items-center gap-1.5 mb-1 mr-1 justify-end">
                                <SpeakerLabel
                                    speaker={speaker}
                                    label={speaker_label}
                                    color={speakerColor!}
                                    onUpdate={onUpdateSpeakerLabel}
                                    meetingId={meetingId}
                                    transcriptId={id}
                                    startTime={timestamp}
                                    endTime={endTime}
                                />
                            <span
                                className="inline-block w-2.5 h-2.5 rounded-full flex-shrink-0"
                                style={{ backgroundColor: speakerColor }}
                            />
                        </div>
                    )}
                    {isStreaming ? (
                        <div
                            className={`bg-emerald-50 border border-emerald-100 rounded-lg px-3 py-2 ${isActive ? 'ring-2 ring-emerald-400' : ''}`}
                            style={hasSpeaker ? { borderRightColor: speakerColor, borderRightWidth: 3 } : undefined}
                        >
                            <p className="text-base text-gray-800 leading-relaxed">{displayText}</p>
                        </div>
                    ) : (
                        <div
                            className={`bg-emerald-50 border border-emerald-100 rounded-lg px-3 py-2 ${isActive ? 'ring-2 ring-emerald-400' : ''}`}
                            style={hasSpeaker ? { borderRightColor: speakerColor, borderRightWidth: 3 } : undefined}
                        >
                            <p className="text-base text-gray-800 leading-relaxed">{displayText}</p>
                        </div>
                    )}
                </div>
                {playButton}
                <Tooltip>
                    <TooltipTrigger>
                        <span className="text-xs text-gray-400 mt-1 flex-shrink-0 min-w-[50px]">
                            {formatRecordingTime(timestamp)}
                        </span>
                    </TooltipTrigger>
                    <TooltipContent>
                        {confidence !== undefined && showConfidence && (
                            <ConfidenceIndicator confidence={confidence} showIndicator={showConfidence} />
                        )}
                    </TooltipContent>
                </Tooltip>
            </div>
        </div>
    );
});

export const VirtualizedTranscriptView: React.FC<VirtualizedTranscriptViewProps> = ({
    segments,
    isRecording = false,
    isPaused = false,
    isProcessing = false,
    isStopping = false,
    enableStreaming = false,
    showConfidence = true,
    disableAutoScroll = false,
    hasMore = false,
    isLoadingMore = false,
    totalCount = 0,
    loadedCount = 0,
    onLoadMore,
    onUpdateSpeakerLabel,
    meetingId,
    onPlayFrom,
    isAudioPlaying = false,
    activeSegmentId = null,
}) => {
    // Create scroll ref first - shared between virtualizer and auto-scroll hook
    const scrollRef = useRef<HTMLDivElement>(null);
    // Ref for infinite scroll trigger element
    const loadMoreTriggerRef = useRef<HTMLDivElement>(null);

    // Force re-render without flushSync (avoids React warning)
    const [, rerender] = useReducer((x: number) => x + 1, 0);

    // Setup virtualizer for efficient rendering of large lists
    const virtualizer = useVirtualizer({
        count: segments.length,
        getScrollElement: () => scrollRef.current,
        estimateSize: () => 60, // Estimated height per segment
        overscan: 10, // Render extra items above/below viewport
        onChange: () => {
            startTransition(() => {
                rerender();
            });
        },
    });

    // Custom hook for auto-scrolling (supports both virtualized and non-virtualized)
    useAutoScroll({
        scrollRef,
        segments,
        isRecording,
        isPaused,
        virtualizer,
        virtualizationThreshold: VIRTUALIZATION_THRESHOLD,
        disableAutoScroll,
    });

    // Streaming text effect hook (typewriter animation for new transcripts)
    const { streamingSegmentId, getDisplayText } = useTranscriptStreaming(
        segments,
        isRecording,
        enableStreaming
    );

    // Infinite scroll: IntersectionObserver to trigger loading more
    useEffect(() => {
        if (!onLoadMore || !hasMore || isLoadingMore || isRecording || segments.length === 0) {
            return;
        }

        const triggerElement = loadMoreTriggerRef.current;
        if (!triggerElement) return;

        const observer = new IntersectionObserver(
            (entries) => {
                if (entries[0].isIntersecting && hasMore && !isLoadingMore) {
                    onLoadMore();
                }
            },
            {
                root: null,
                rootMargin: '100px',
                threshold: 0,
            }
        );

        observer.observe(triggerElement);

        return () => observer.disconnect();
    }, [hasMore, isLoadingMore, onLoadMore, isRecording, segments.length]);

    // Scroll-based fallback for fast scrolling
    useEffect(() => {
        if (!onLoadMore || !hasMore || isLoadingMore || isRecording) return;

        const scrollElement = scrollRef.current;
        if (!scrollElement) return;

        let ticking = false;

        const handleScroll = () => {
            if (ticking || isLoadingMore || !hasMore) return;

            ticking = true;
            requestAnimationFrame(() => {
                const { scrollTop, scrollHeight, clientHeight } = scrollElement;
                const scrollBottom = scrollHeight - scrollTop - clientHeight;

                // Trigger load when within 200px of bottom
                if (scrollBottom < 200 && hasMore && !isLoadingMore) {
                    onLoadMore();
                }
                ticking = false;
            });
        };

        scrollElement.addEventListener('scroll', handleScroll, { passive: true });
        return () => scrollElement.removeEventListener('scroll', handleScroll);
    }, [onLoadMore, hasMore, isLoadingMore, isRecording]);

    // Use simple rendering for small lists, virtualization for large lists
    const useVirtualization = segments.length >= VIRTUALIZATION_THRESHOLD;

    return (
        <div ref={scrollRef} className="flex flex-col h-full overflow-y-auto px-4 py-2">
            {/* Recording Status Bar - Sticky at top, always visible when recording */}
            <AnimatePresence>
                {isRecording && (
                    <div className="sticky top-0 z-10 bg-white pb-2">
                        <RecordingStatusBar isPaused={isPaused} />
                    </div>
                )}
            </AnimatePresence>

            {/* Content - add padding when recording to prevent overlap */}
            <div className={isRecording ? 'pt-2' : ''}>
            {segments.length === 0 ? (
                // Empty state
                <motion.div
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    className="text-center text-gray-500 mt-8"
                >
                    {isRecording ? (
                        <>
                            <div className="flex items-center justify-center mb-3">
                                <div className={`w-3 h-3 rounded-full ${isPaused ? 'bg-orange-500' : 'bg-blue-500 animate-pulse'}`}></div>
                            </div>
                            <p className="text-sm text-gray-600">
                                {isPaused ? 'Recording paused' : 'Listening for speech...'}
                            </p>
                            <p className="text-xs mt-1 text-gray-400">
                                {isPaused ? 'Click resume to continue recording' : 'Speak to see live transcription'}
                            </p>
                        </>
                    ) : (
                        <>
                            <p className="text-lg font-semibold">Welcome to meetily!</p>
                            <p className="text-xs mt-1">Start recording to see live transcription</p>
                        </>
                    )}
                </motion.div>
            ) : useVirtualization ? (
                // Virtualized rendering for large lists
                <>
                    <div
                        style={{
                            height: virtualizer.getTotalSize(),
                            width: "100%",
                            position: "relative",
                        }}
                    >
                        {virtualizer.getVirtualItems().map((virtualRow) => {
                            const segment = segments[virtualRow.index];
                            const isStreaming = streamingSegmentId === segment.id;

                            return (
                                <div
                                    key={segment.id}
                                    data-index={virtualRow.index}
                                    ref={virtualizer.measureElement}
                                    style={{
                                        position: "absolute",
                                        top: 0,
                                        left: 0,
                                        width: "100%",
                                        transform: `translateY(${virtualRow.start}px)`,
                                    }}
                                >
                                    <TranscriptSegment
                                        id={segment.id}
                                        timestamp={segment.timestamp}
                                        endTime={segment.endTime}
                                        text={getDisplayText(segment)}
                                        confidence={segment.confidence}
                                        isStreaming={isStreaming}
                                        showConfidence={showConfidence}
                                        source_device={segment.source_device}
                                        speaker={segment.speaker}
                                        speaker_label={segment.speaker_label}
                                        hasAudioTime={segment.hasAudioTime ?? false}
                                        onUpdateSpeakerLabel={onUpdateSpeakerLabel}
                                        meetingId={meetingId}
                                        onPlayFrom={onPlayFrom}
                                        isActive={activeSegmentId === segment.id}
                                        isAudioPlaying={isAudioPlaying}
                                    />
                                </div>
                            );
                        })}
                    </div>

                    {/* Infinite scroll trigger and loading indicator */}
                    {(hasMore || isLoadingMore) && !isRecording && segments.length > 0 && (
                        <div ref={loadMoreTriggerRef} className="flex justify-center items-center py-4 mt-2">
                            {isLoadingMore ? (
                                <div className="flex items-center gap-2 text-gray-500">
                                    <div className="w-4 h-4 border-2 border-gray-300 border-t-gray-600 rounded-full animate-spin" />
                                    <span className="text-sm">Loading more...</span>
                                </div>
                            ) : hasMore && totalCount > 0 ? (
                                <span className="text-sm text-gray-400">
                                    Showing {loadedCount} of {totalCount} segments
                                </span>
                            ) : null}
                        </div>
                    )}

                    {/* Listening indicator when recording */}
                    {!isStopping && isRecording && !isPaused && !isProcessing && segments.length > 0 && (
                        <motion.div
                            initial={{ opacity: 0 }}
                            animate={{ opacity: 1 }}
                            exit={{ opacity: 0 }}
                            className="flex items-center gap-2 mt-4 text-gray-500"
                        >
                            <div className="w-2 h-2 bg-blue-500 rounded-full animate-pulse"></div>
                            <span className="text-sm">Listening...</span>
                        </motion.div>
                    )}
                </>
            ) : (
                // Simple rendering for small lists (better animations)
                <>
                    <div className="space-y-1">
                        {segments.map((segment) => {
                            const isStreaming = streamingSegmentId === segment.id;

                            return (
                                <motion.div
                                    key={segment.id}
                                    initial={{ opacity: 0, y: 5 }}
                                    animate={{ opacity: 1, y: 0 }}
                                    transition={{ duration: 0.15 }}
                                >
                                    <TranscriptSegment
                                        id={segment.id}
                                        timestamp={segment.timestamp}
                                        endTime={segment.endTime}
                                        text={getDisplayText(segment)}
                                        confidence={segment.confidence}
                                        isStreaming={isStreaming}
                                        showConfidence={showConfidence}
                                        source_device={segment.source_device}
                                        speaker={segment.speaker}
                                        speaker_label={segment.speaker_label}
                                        hasAudioTime={segment.hasAudioTime ?? false}
                                        onUpdateSpeakerLabel={onUpdateSpeakerLabel}
                                        meetingId={meetingId}
                                        onPlayFrom={onPlayFrom}
                                        isActive={activeSegmentId === segment.id}
                                        isAudioPlaying={isAudioPlaying}
                                    />
                                </motion.div>
                            );
                        })}
                    </div>

                    {/* Infinite scroll trigger (for small lists that grow) */}
                    {(hasMore || isLoadingMore) && !isRecording && segments.length > 0 && (
                        <div ref={loadMoreTriggerRef} className="flex justify-center items-center py-4 mt-2">
                            {isLoadingMore ? (
                                <div className="flex items-center gap-2 text-gray-500">
                                    <div className="w-4 h-4 border-2 border-gray-300 border-t-gray-600 rounded-full animate-spin" />
                                    <span className="text-sm">Loading more...</span>
                                </div>
                            ) : hasMore && totalCount > 0 ? (
                                <span className="text-sm text-gray-400">
                                    Showing {loadedCount} of {totalCount} segments
                                </span>
                            ) : null}
                        </div>
                    )}

                    {/* Listening indicator when recording */}
                    {!isStopping && isRecording && !isPaused && !isProcessing && segments.length > 0 && (
                        <motion.div
                            initial={{ opacity: 0 }}
                            animate={{ opacity: 1 }}
                            exit={{ opacity: 0 }}
                            className="flex items-center gap-2 mt-4 text-gray-500"
                        >
                            <div className="w-2 h-2 bg-blue-500 rounded-full animate-pulse"></div>
                            <span className="text-sm">Listening...</span>
                        </motion.div>
                    )}
                </>
            )}
            </div>
        </div>
    );
};
