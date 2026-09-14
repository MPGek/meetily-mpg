import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { MeetingTag, MeetingTagWithUsage } from '@/lib/meeting-tags';

/**
 * Pending tag set for the upcoming / in-progress recording (change:
 * tags-before-during-recording). The backend `metadata.json` key is the
 * source of truth (survives UI reload); this hook mirrors it locally for
 * responsiveness and syncs every change back.
 */
export function usePendingRecordingTags(isRecording: boolean) {
  const [pending, setPending] = useState<MeetingTag[]>([]);
  const pendingRef = useRef<MeetingTag[]>(pending);
  const isRecordingRef = useRef(isRecording);
  const syncChain = useRef<Promise<unknown>>(Promise.resolve());

  useEffect(() => {
    pendingRef.current = pending;
  }, [pending]);
  useEffect(() => {
    isRecordingRef.current = isRecording;
  }, [isRecording]);

  const load = useCallback(async () => {
    try {
      const ids = await invoke<string[]>('get_recording_pending_tags');
      if (ids.length === 0) {
        setPending([]);
        return;
      }
      const all = await invoke<MeetingTagWithUsage[]>('list_tags');
      const byId = new Map(all.map((t) => [t.id, t]));
      setPending(ids.map((id) => byId.get(id)).filter((t): t is MeetingTagWithUsage => !!t));
    } catch {
      // No active recording (pre-start with nothing picked yet) — empty set.
      setPending([]);
    }
  }, []);

  // On recording start, push the pre-start selection so it travels with the
  // session (the backend initializes an empty set at start), then confirm the
  // canonical set. Without a local selection this is a plain reload; clear
  // local state when recording ends.
  useEffect(() => {
    if (isRecording) {
      const ids = pendingRef.current.map((t) => t.id);
      (async () => {
        if (ids.length > 0) {
          for (const delay of [0, 300]) {
            if (delay > 0) await new Promise((r) => setTimeout(r, delay));
            try {
              await invoke('set_recording_pending_tags', { tagIds: ids });
              break;
            } catch (e) {
              console.error('Failed to carry pre-start pending tags into recording:', e);
            }
          }
        }
        await load();
      })();
    } else {
      setPending([]);
    }
  }, [isRecording, load]);

  // Initial load covers reload-UI-mid-recording: backend still has the set.
  useEffect(() => {
    load();
  }, [load]);

  // Serialized immediate writes: no debounce timer that can be dropped when
  // recording stops or the page unmounts; the chain keeps last-write-wins
  // ordering. Pre-start edits stay local — the start transition pushes them.
  const sync = useCallback((ids: string[]) => {
    if (!isRecordingRef.current) return;
    syncChain.current = syncChain.current
      .catch(() => {})
      .then(() => invoke<string[]>('set_recording_pending_tags', { tagIds: ids }))
      .catch((e) => console.error('Failed to sync pending tags:', e));
  }, []);

  const toggle = useCallback(
    (tag: MeetingTag) => {
      setPending((prev) => {
        const next = prev.some((t) => t.id === tag.id)
          ? prev.filter((t) => t.id !== tag.id)
          : [...prev, tag];
        sync(next.map((t) => t.id));
        return next;
      });
    },
    [sync]
  );

  const create = useCallback(
    async (name: string): Promise<void> => {
      const tag = await invoke<MeetingTag>('create_tag', { name, color: null });
      setPending((prev) => {
        if (prev.some((t) => t.id === tag.id)) return prev;
        const next = [...prev, tag];
        sync(next.map((t) => t.id));
        return next;
      });
    },
    [sync]
  );

  const remove = useCallback(
    (tagId: string) => {
      setPending((prev) => {
        const next = prev.filter((t) => t.id !== tagId);
        sync(next.map((t) => t.id));
        return next;
      });
    },
    [sync]
  );

  return { pending, toggle, create, remove, reload: load };
}
