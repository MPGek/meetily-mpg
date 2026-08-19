import type { SpeakerTurn } from '@/services/recordingService';
import type { Transcript } from '@/types';

/**
 * Pure helpers for live speaker-label re-matching and binding rewrite.
 * Kept free of React so the live label logic is unit-testable.
 */

/**
 * Assign a live transcript segment a speaker by greatest temporal overlap
 * against emitted speaker turns from the same channel. Mirrors the overlap
 * pass of the backend `find_best_speaker` (without its stop-time gap-fill).
 * Returns the matching cluster label plus recognized/user-bound display name
 * and provenance (if the turn carries one).
 */
export function matchSpeakerToTranscript(
  segment: Pick<Transcript, 'audio_start_time' | 'audio_end_time' | 'source_device'>,
  turns: SpeakerTurn[]
): { speaker: string; displayName?: string; matchedBy?: string; matchScore?: number } | undefined {
  const tStart = segment.audio_start_time ?? 0;
  const tEnd = segment.audio_end_time ?? tStart;

  let bestSpeaker: string | undefined;
  let bestDisplayName: string | undefined;
  let bestMatchedBy: string | undefined;
  let bestMatchScore: number | undefined;
  let bestOverlap = 0;
  for (const turn of turns) {
    if (turn.source_device !== segment.source_device) {
      continue;
    }
    const overlapStart = Math.max(tStart, turn.start_time);
    const overlapEnd = Math.min(tEnd, turn.end_time);
    if (overlapStart < overlapEnd) {
      const overlap = overlapEnd - overlapStart;
      if (overlap > bestOverlap) {
        bestOverlap = overlap;
        bestSpeaker = turn.speaker;
        bestDisplayName = turn.display_name;
        bestMatchedBy = turn.matched_by;
        bestMatchScore = turn.match_score;
      }
    }
  }
  return bestSpeaker
    ? { speaker: bestSpeaker, displayName: bestDisplayName, matchedBy: bestMatchedBy, matchScore: bestMatchScore }
    : undefined;
}

/**
 * Rewrite every live turn matching `clusterLabel` to carry the user's assigned
 * name with user provenance, dropping any stale auto-recognition score. Returns
 * a new array (idempotent: applying twice yields the same result).
 */
export function rewriteTurnsForBinding(
  turns: SpeakerTurn[],
  clusterLabel: string,
  name: string
): SpeakerTurn[] {
  return turns.map(t =>
    t.speaker === clusterLabel
      ? { ...t, display_name: name, matched_by: 'user' as const, match_score: undefined }
      : t
  );
}

/**
 * Rewrite only the turn(s) of `clusterLabel` whose time window overlaps
 * `[start, end]`, typically the single block a user edited. Other turns of the
 * cluster are left unchanged. Returns a new array.
 */
export function rewriteTurnsInWindow(
  turns: SpeakerTurn[],
  clusterLabel: string,
  sourceDevice: string | undefined,
  start: number,
  end: number,
  name: string
): SpeakerTurn[] {
  return turns.map(t =>
    t.speaker === clusterLabel
      && t.source_device === sourceDevice
      && t.start_time < end && t.end_time > start
      ? { ...t, display_name: name, matched_by: 'user' as const, match_score: undefined }
      : t
  );
}

/**
 * Re-match a transcript list against a turn stream, freezing any transcript
 * present in `assigned` (a Map keyed by transcript id). Assigned transcripts
 * are user-owned and never re-matched, so stale auto turns cannot revert them.
 * Returns a new list only when something changed, else the input list.
 */
export function rematchTranscripts(
  transcripts: Transcript[],
  turns: SpeakerTurn[],
  assigned: ReadonlyMap<string, unknown>
): Transcript[] {
  let changed = false;
  const next = transcripts.map(t => {
    if (assigned.has(t.id)) return t;
    const matched = matchSpeakerToTranscript(t, turns);
    if (!matched) return t;
    const displayName = matched.displayName || (matched.speaker === t.speaker ? t.speaker_label : undefined);
    if (matched.speaker !== t.speaker || (displayName && displayName !== t.speaker_label)) {
      changed = true;
      return {
        ...t,
        speaker: matched.speaker,
        speaker_label: displayName ?? t.speaker_label,
        speaker_matched_by: matched.matchedBy,
        speaker_match_score: matched.matchScore,
      };
    }
    return t;
  });
  return changed ? next : transcripts;
}
