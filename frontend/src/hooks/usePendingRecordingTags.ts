import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import type { MeetingTag, MeetingTagWithUsage } from '@/lib/meeting-tags';

/** Retry delays for carrying the pre-start set into a fresh recording. */
const CARRY_RETRY_DELAYS_MS = [0, 300, 900];

/**
 * Pending tag set for the upcoming / in-progress recording. The backend
 * `metadata.json` key is the source of truth (survives UI reload); this hook
 * mirrors it locally for responsiveness and syncs every change back.
 *
 * A read or write failure never clears the visible selection, and the
 * start transition confirms the pushed set from the write response instead
 * of re-reading and overwriting it (change: tags-persistence-and-palette).
 */
export function usePendingRecordingTags(isRecording: boolean) {
  const [pending, setPending] = useState<MeetingTag[]>([]);
  const pendingRef = useRef<MeetingTag[]>(pending);
  const isRecordingRef = useRef(isRecording);
  const syncChain = useRef<Promise<unknown>>(Promise.resolve());
  const warnedRef = useRef(false);

  useEffect(() => {
    pendingRef.current = pending;
  }, [pending]);
  useEffect(() => {
    isRecordingRef.current = isRecording;
  }, [isRecording]);

  const warnPersistFailure = useCallback(() => {
    if (warnedRef.current) return;
    warnedRef.current = true;
    toast.warning('Could not save tags for this recording', {
      description: 'Your selection is kept, but it may not be linked to the saved meeting.',
    });
  }, []);

  const resolveIds = useCallback(async (ids: string[]) => {
    const all = await invoke<MeetingTagWithUsage[]>('list_tags');
    const byId = new Map(all.map((t) => [t.id, t]));
    return ids.map((id) => byId.get(id)).filter((t): t is MeetingTagWithUsage => !!t);
  }, []);

  // Read the stored set. While recording, an empty or failed read never
  // clears an existing selection; when idle it just resets to empty.
  const load = useCallback(async () => {
    try {
      const ids = await invoke<string[]>('get_recording_pending_tags');
      if (ids.length === 0) {
        if (!isRecordingRef.current) setPending([]);
        return;
      }
      const resolved = await resolveIds(ids);
      if (resolved.length > 0) setPending(resolved);
    } catch {
      if (!isRecordingRef.current) setPending([]);
    }
  }, [resolveIds]);

  // On recording start, push the pre-start selection so it travels with the
  // session, then confirm from the write response. Without a local selection
  // this is a reload of the backend set. Clear local state when recording ends.
  useEffect(() => {
    if (!isRecording) {
      warnedRef.current = false;
      setPending([]);
      return;
    }

    const ids = pendingRef.current.map((t) => t.id);
    if (ids.length === 0) {
      void load();
      return;
    }

    void (async () => {
      for (const delay of CARRY_RETRY_DELAYS_MS) {
        if (delay > 0) await new Promise((resolve) => setTimeout(resolve, delay));
        try {
          const canonical = await invoke<string[]>('set_recording_pending_tags', {
            tagIds: ids,
          });
          if (canonical.length === 0) {
            // The write was accepted but dropped every id: keep the local set.
            warnPersistFailure();
            return;
          }
          const resolved = await resolveIds(canonical);
          if (resolved.length > 0) setPending(resolved);
          return;
        } catch {
          // Retry: the recording folder may not exist yet.
        }
      }
      warnPersistFailure();
    })();
  }, [isRecording, load, resolveIds, warnPersistFailure]);

  useEffect(() => {
    void load();
  }, [load]);

  // Serialized immediate writes: no debounce timer that can be dropped when
  // recording stops or the page unmounts; the chain keeps last-write-wins
  // ordering. Failures keep the local selection and warn once.
  const sync = useCallback(
    (ids: string[]) => {
      if (!isRecordingRef.current) return;
      syncChain.current = syncChain.current
        .catch(() => {})
        .then(() => invoke<string[]>('set_recording_pending_tags', { tagIds: ids }))
        .then((canonical) => {
          if (canonical.length === 0 && ids.length > 0) warnPersistFailure();
        })
        .catch((e) => {
          console.error('Failed to sync pending tags:', e);
          warnPersistFailure();
        });
    },
    [warnPersistFailure]
  );

  const applySelection = useCallback(
    (next: MeetingTag[]) => {
      pendingRef.current = next;
      setPending(next);
      sync(next.map((t) => t.id));
    },
    [sync]
  );

  const toggle = useCallback(
    (tag: MeetingTag) => {
      const current = pendingRef.current;
      const next = current.some((t) => t.id === tag.id)
        ? current.filter((t) => t.id !== tag.id)
        : [...current, tag];
      applySelection(next);
    },
    [applySelection]
  );

  const create = useCallback(
    async (name: string): Promise<void> => {
      const tag = await invoke<MeetingTag>('create_tag', { name, color: null });
      const current = pendingRef.current;
      if (current.some((t) => t.id === tag.id)) return;
      applySelection([...current, tag]);
    },
    [applySelection]
  );

  const remove = useCallback(
    (tagId: string) => {
      applySelection(pendingRef.current.filter((t) => t.id !== tagId));
    },
    [applySelection]
  );

  return { pending, toggle, create, remove, reload: load };
}
