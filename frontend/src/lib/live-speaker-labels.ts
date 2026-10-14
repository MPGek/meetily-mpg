import type { SpeakerTurn } from '@/services/recordingService';
import type { LiveBlockPart, LiveTranscriptBlock, LiveTranscriptBlocks, Transcript } from '@/types';

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
 * present in `assigned` (a Set or Map keyed by transcript id). Assigned
 * transcripts are user-owned and never re-matched, so stale auto turns cannot
 * revert them. Returns a new list only when something changed, else the input
 * list.
 */
export function rematchTranscripts(
  transcripts: Transcript[],
  turns: SpeakerTurn[],
  assigned: { has(id: string): boolean }
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

/**
 * Store the latest display revision of a transcript block's live word-level
 * diarization sub-rows. Revisions are per parent sequence_id and monotonic: a
 * stale (older-or-equal) revision is ignored, so re-attribution replaces the
 * previous rendering instead of stacking rows. Returns the input map when
 * nothing changed.
 */
export function upsertLiveBlocks(
  prev: Map<number, LiveTranscriptBlocks>,
  payload: LiveTranscriptBlocks
): Map<number, LiveTranscriptBlocks> {
  const existing = prev.get(payload.parent_sequence_id);
  if (existing && existing.revision >= payload.revision) return prev;
  const next = new Map(prev);
  next.set(payload.parent_sequence_id, payload);
  return next;
}

/** Resolution inputs for sub-row user assignments (live-speaker-labels delta). */
export interface LiveBlockResolution {
  /** Cluster-wide (apply-to-all) label bindings: cluster label -> name. */
  clusterBindings?: ReadonlyMap<string, string>;
  /** Parent-level single-block pin for the transcript being rendered. */
  pinned?: { cluster: string; name: string };
  /** Window-scoped per-turn overrides, oldest first. */
  windowOverrides?: ReadonlyArray<LiveWindowOverride>;
  /** Channel of the block being resolved; overrides of another channel skip it. */
  sourceDevice?: string;
}

/** A user's single sub-row assignment: the time window it covers on a channel. */
export interface LiveWindowOverride {
  /** Cluster the row showed when edited (kept for reference, not matched on). */
  cluster: string;
  sourceDevice?: string;
  start: number;
  end: number;
  name: string;
}

/** A user-named copy of a block, or the block itself when nothing changes. */
function withUserName(b: LiveTranscriptBlock, name: string | undefined): LiveTranscriptBlock {
  if (name === undefined) return b;
  if (name === b.display_name && b.matched_by === 'user') return b;
  return { ...b, display_name: name, matched_by: 'user' };
}

/**
 * Apply the cluster-wide live assignments: a parent pin and a cluster binding
 * name every sub-row of their cluster; the pin wins over the binding.
 */
export function applyClusterAssignments(
  blocks: LiveTranscriptBlock[],
  ctx: LiveBlockResolution
): LiveTranscriptBlock[] {
  return blocks.map(b =>
    withUserName(
      b,
      ctx.pinned && ctx.pinned.cluster === b.speaker ? ctx.pinned.name : ctx.clusterBindings?.get(b.speaker)
    )
  );
}

/**
 * Apply window-scoped overrides: each lands on the sub-row whose time window
 * it covers most on the same channel, whatever cluster that window is now
 * attributed to, and the most recent override wins.
 */
export function applyWindowOverrides(
  blocks: LiveTranscriptBlock[],
  ctx: LiveBlockResolution
): LiveTranscriptBlock[] {
  // Each override lands on the one sub-row it overlaps most, so a boundary
  // shift between revisions cannot spill it onto a neighbour. Later overrides
  // replace earlier ones on the same sub-row.
  const landed = new Map<number, LiveWindowOverride>();
  for (const o of ctx.windowOverrides ?? []) {
    if (o.sourceDevice && ctx.sourceDevice && o.sourceDevice !== ctx.sourceDevice) continue;
    let best = -1;
    let bestOverlap = 0;
    blocks.forEach((b, i) => {
      const overlap = Math.min(o.end, b.end) - Math.max(o.start, b.start);
      if (overlap > bestOverlap) {
        bestOverlap = overlap;
        best = i;
      }
    });
    if (best >= 0) landed.set(best, o);
  }
  return blocks.map((b, i) => withUserName(b, landed.get(i)?.name));
}

/**
 * Apply live user assignments across a block's sub-rows: a window-scoped
 * override beats a parent pin, which beats a cluster binding. Returns new
 * block objects only for changed rows, so unchanged rows keep their identity.
 */
export function resolveLiveBlocks(
  blocks: LiveTranscriptBlock[],
  ctx: LiveBlockResolution
): LiveTranscriptBlock[] {
  return applyWindowOverrides(applyClusterAssignments(blocks, ctx), ctx);
}

/**
 * The live view of a split block: user assignments applied and adjacent
 * sub-rows of the same visible speaker merged. Cluster assignments go first
 * (they are exact per cluster), then the merge, so a window override made on a
 * merged row lands on the whole merged span; a second merge joins neighbours
 * that a rename made equal.
 */
export function resolveMergedLiveBlocks(
  blocks: LiveTranscriptBlock[],
  ctx: LiveBlockResolution
): LiveTranscriptBlock[] {
  return mergeLiveBlocks(applyWindowOverrides(mergeLiveBlocks(applyClusterAssignments(blocks, ctx)), ctx));
}

/** Whether two sub-rows show the same speaker: same visible name, same ownership. */
function sameVisibleSpeaker(a: LiveTranscriptBlock, b: LiveTranscriptBlock): boolean {
  return (
    (a.display_name ?? a.speaker) === (b.display_name ?? b.speaker) &&
    (a.matched_by === 'user') === (b.matched_by === 'user')
  );
}

function partsOf(b: LiveTranscriptBlock): LiveBlockPart[] {
  return b.parts ?? [{ speaker: b.speaker, start: b.start, end: b.end }];
}

/**
 * Merge adjacent sub-rows that show the same speaker (same visible name and
 * ownership) into one. A merged row spans its first part's start to its last
 * part's end, joins the text, keeps the longest part's cluster (dot colour)
 * and the lowest match score, and lists its parts so an edit can reach every
 * constituent cluster. A row that merges nothing is returned as is.
 */
export function mergeLiveBlocks(blocks: LiveTranscriptBlock[]): LiveTranscriptBlock[] {
  const out: LiveTranscriptBlock[] = [];
  let group: LiveTranscriptBlock[] = [];
  const flush = () => {
    if (group.length === 1) {
      out.push(group[0]);
    } else if (group.length > 1) {
      const parts = group.flatMap(partsOf);
      const longest = parts.reduce((a, p) => (p.end - p.start > a.end - a.start ? p : a));
      const scores = group.map(b => b.match_score).filter((s): s is number => s !== undefined);
      out.push({
        start: group[0].start,
        end: group[group.length - 1].end,
        text: group.map(b => b.text).join(' '),
        speaker: longest.speaker,
        display_name: group[0].display_name,
        matched_by: group[0].matched_by,
        match_score: scores.length ? Math.min(...scores) : undefined,
        parts,
      });
    }
    group = [];
  };
  for (const b of blocks) {
    if (group.length && !sameVisibleSpeaker(group[group.length - 1], b)) flush();
    group.push(b);
  }
  flush();
  return out;
}

/** One backend live-speaker assignment an edit has to send. */
export type LiveAssignmentCall =
  | { kind: 'cluster'; cluster: string }
  | { kind: 'block'; cluster: string; start: number; end?: number };

/**
 * The backend assignments for a live label edit. The stop-time persistence
 * matches per-turn overrides by cluster and window, so a row merged from
 * several clusters needs one call per part: per distinct cluster for a
 * cluster-wide edit, per part over its own window for a single-row edit.
 */
export function liveAssignmentCalls(
  parts: LiveBlockPart[] | undefined,
  cluster: string,
  clusterWide: boolean,
  start: number,
  end?: number
): LiveAssignmentCall[] {
  if (clusterWide) {
    const clusters = parts?.length ? [...new Set(parts.map(p => p.speaker))] : [cluster];
    return clusters.map(c => ({ kind: 'cluster', cluster: c }));
  }
  if (!parts?.length) return [{ kind: 'block', cluster, start, end }];
  return parts.map(p => ({ kind: 'block', cluster: p.speaker, start: p.start, end: p.end }));
}
