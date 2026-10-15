// Suspect-prototype display rules for the voiceprint browser
// (guard-prototype-enrollment). The backend computes `suspect` on read; these
// helpers only decide what the browser shows.

export type SuspectFields = {
  is_verified: number;
  suspect?: boolean;
};

export type SuspectBadge = 'suspect' | 'acknowledged' | null;

// A verified suspect row keeps its badge but reads as acknowledged: the user
// already listened to it, so it stops asking for attention (never removed).
export function suspectBadge(row: SuspectFields): SuspectBadge {
  if (!row.suspect) return null;
  return row.is_verified !== 0 ? 'acknowledged' : 'suspect';
}

export function visibleRows<T extends SuspectFields>(
  rows: T[],
  opts: { hideVerified: boolean; suspectOnly: boolean },
): T[] {
  return rows.filter((r) => {
    if (opts.hideVerified && r.is_verified !== 0) return false;
    if (opts.suspectOnly && !r.suspect) return false;
    return true;
  });
}

// Speakers with no suspect row drop out of the list when the filter is on.
export function hasSuspect(rows: SuspectFields[]): boolean {
  return rows.some((r) => !!r.suspect);
}
