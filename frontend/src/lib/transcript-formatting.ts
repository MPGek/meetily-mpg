import type { Transcript } from '@/types';

/**
 * Appends `next` and re-sorts by `sequence_id`, unless a transcript with the
 * same `text` + `timestamp` already exists, in which case `prev` itself is
 * returned unchanged (lifted from `TranscriptContext.addTranscript`).
 */
export function dedupeAndInsertTranscript(prev: Transcript[], next: Transcript): Transcript[] {
  // Check if this transcript already exists
  const exists = prev.some(
    t => t.text === next.text && t.timestamp === next.timestamp
  );
  if (exists) {
    return prev;
  }

  // Add new transcript and sort by sequence_id to maintain order
  const updated = [...prev, next];
  return updated.sort((a, b) => (a.sequence_id || 0) - (b.sequence_id || 0));
}

// Format timestamps as recording-relative [MM:SS] instead of wall-clock time
function formatTime(seconds: number | undefined): string {
  if (seconds === undefined) return '[--:--]';
  const totalSecs = Math.floor(seconds);
  const mins = Math.floor(totalSecs / 60);
  const secs = totalSecs % 60;
  return `[${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}]`;
}

/** One `[MM:SS] text` line per transcript, as copied to the clipboard. */
export function formatTranscriptForClipboard(transcripts: Transcript[]): string {
  return transcripts
    .map(t => `${formatTime(t.audio_start_time)} ${t.text}`)
    .join('\n');
}
