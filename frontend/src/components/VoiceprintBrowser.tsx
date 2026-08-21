'use client';

import React, { useEffect, useState, useCallback, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useAudioPlayer } from '@/hooks/useAudioPlayer';
import { ChevronDown, ChevronRight, ChevronsDown, ChevronsUp } from 'lucide-react';

type VoiceprintRow = {
  id: string;
  model: string;
  channel: string;
  duration_secs: number;
  speaker_id: string | null;
  meeting_id: string | null;
  cluster_label: string | null;
  audio_start_time: number | null;
  audio_end_time: number | null;
  meeting_title: string | null;
  created_at: string;
};

type SpeakerVoiceprints = {
  speaker_id: string;
  speaker_name: string;
  is_me: boolean;
  prototype_count: number;
  prototypes: VoiceprintRow[];
};

type MeetingVoiceprints = {
  meeting_id: string;
  meeting_title: string;
  caches: VoiceprintRow[];
};

type VoiceprintBrowserData = {
  speakers: SpeakerVoiceprints[];
  unconfirmed: MeetingVoiceprints[];
};

type StorageStats = {
  registry_count: number;
  prototype_count: number;
  cache_count: number;
  total_bytes: number;
};

type SpeakerLite = { id: string; name: string; is_me?: boolean };

// Human-readable size formatter, kept identical to the one used by the
// Settings general-tab storage section (DiarizationSettings.tsx) so the two
// surfaces agree on how embedding size is displayed.
function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(1)} GB`;
}


// ——— Shared person picker (same as speaker name change / assignment) ———
function PersonPickerDialog({
  open,
  onClose,
  speakers,
  excludeId,
  allowAnonymous,
  title,
  onSelect,
  onCreate,
  onAnonymous,
}: {
  open: boolean;
  onClose: () => void;
  speakers: SpeakerLite[];
  excludeId?: string;
  allowAnonymous?: boolean;
  title: string;
  onSelect: (id: string) => void;
  onCreate: (name: string) => void;
  onAnonymous?: () => void;
}) {
  const [query, setQuery] = useState('');
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    if (open) setQuery('');
  }, [open]);

  const filtered = useMemo(() => {
    let list = speakers;
    if (excludeId) list = list.filter((s) => s.id !== excludeId);
    if (!query.trim()) return list;
    const q = query.toLowerCase();
    return list.filter((s) => s.name.toLowerCase().includes(q));
  }, [speakers, query, excludeId]);

  const exactMatch = useMemo(
    () => speakers.some((s) => s.name.toLowerCase() === query.trim().toLowerCase()),
    [speakers, query]
  );

  const handleCreate = async () => {
    const name = query.trim();
    if (!name || exactMatch) return;
    setCreating(true);
    try {
      await onCreate(name);
      onClose();
    } finally {
      setCreating(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' && query.trim() && !exactMatch) {
      e.preventDefault();
      handleCreate();
    } else if (e.key === 'Escape') {
      onClose();
    }
  };

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/40" onClick={onClose} aria-hidden />
      <div role="dialog" aria-modal="true" aria-label={title} className="relative bg-white rounded-lg shadow-xl w-[380px] max-h-[70vh] flex flex-col border">
        <div className="px-4 py-3 border-b">
          <h4 className="font-semibold text-sm">{title}</h4>
        </div>
        <div className="px-4 py-2 border-b">
          <input
            autoFocus
            placeholder="Search or type name..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            className="w-full text-sm bg-transparent outline-none placeholder:text-gray-400 border px-2 py-1.5 rounded"
          />
        </div>
        <div className="flex-1 overflow-y-auto">
          {allowAnonymous && (
            <button
              className="w-full text-left px-4 py-2 text-sm hover:bg-gray-100 border-b"
              onClick={() => {
                onAnonymous?.();
                onClose();
              }}
            >
              <span className="italic text-gray-600">Anonymous (no speaker)</span>
            </button>
          )}
          {filtered.length > 0 ? (
            filtered.map((sp) => (
              <button
                key={sp.id}
                className="w-full text-left px-4 py-2 text-sm hover:bg-gray-100 flex items-center gap-2"
                onClick={() => {
                  onSelect(sp.id);
                  onClose();
                }}
              >
                <span className="truncate">{sp.name}</span>
              </button>
            ))
          ) : (
            <div className="px-4 py-3 text-sm text-gray-400">
              {query.trim() ? `Press Enter to create "${query.trim()}"` : 'No speakers yet'}
            </div>
          )}
        </div>
        {query.trim() && !exactMatch && (
          <div className="border-t px-4 py-2 flex justify-between items-center">
            <span className="text-xs text-gray-500 truncate">Create &quot;{query.trim()}&quot;</span>
            <button
              disabled={creating}
              className="text-xs px-3 py-1 bg-blue-600 text-white rounded disabled:opacity-50"
              onClick={handleCreate}
            >
              {creating ? 'Creating…' : 'Create'}
            </button>
          </div>
        )}
        <div className="border-t px-4 py-2 flex justify-end">
          <button className="text-xs px-3 py-1 bg-gray-100 rounded" onClick={onClose}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

function ConfirmReplaceDialog({
  open,
  onClose,
  onConfirm,
  sourceName,
  targetName,
  preview,
}: {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  sourceName: string;
  targetName: string | null;
  preview: { affected_meetings: number; affected_clusters: number; affected_transcripts: number } | null;
}) {
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/40" onClick={onClose} aria-hidden />
      <div role="dialog" aria-modal="true" aria-label="Confirm replacement" className="relative bg-white rounded-lg shadow-xl w-[420px] border p-4">
        <h4 className="font-semibold text-sm mb-2">Confirm whole-corpus replacement</h4>
        <p className="text-sm text-gray-600 mb-3">
          Replace speaker <span className="font-medium">{sourceName}</span> with{' '}
          <span className="font-medium">{targetName ?? 'anonymous'}</span>?
        </p>
        {preview ? (
          <div className="bg-amber-50 border border-amber-200 rounded p-3 text-xs mb-3">
            <div>Affected meetings: {preview.affected_meetings}</div>
            <div>Affected clusters: {preview.affected_clusters}</div>
            <div>Affected transcripts: {preview.affected_transcripts}</div>
            <div className="text-gray-500 mt-1">User-bound clusters and per-block overrides will be preserved.</div>
          </div>
        ) : (
          <div className="text-xs text-gray-400 mb-3">Loading impact…</div>
        )}
        <div className="flex justify-end gap-2">
          <button className="text-xs px-3 py-1.5 bg-gray-100 rounded" onClick={onClose}>
            Cancel
          </button>
          <button className="text-xs px-3 py-1.5 bg-amber-600 text-white rounded" onClick={onConfirm}>
            Confirm replace
          </button>
        </div>
      </div>
    </div>
  );
}

export default function VoiceprintBrowser() {
  const [data, setData] = useState<VoiceprintBrowserData | null>(null);
  const [stats, setStats] = useState<StorageStats | null>(null);
  const [audioPath, setAudioPath] = useState<string | null>(null);
  const [pendingRange, setPendingRange] = useState<{ start: number; end: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [speakersList, setSpeakersList] = useState<SpeakerLite[]>([]);
  const player = useAudioPlayer(audioPath);

  // Collapse/expand state — set of expanded group ids: `speaker:${id}` / `meeting:${id}`
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  const load = useCallback(async () => {
    try {
      const browser = await invoke<VoiceprintBrowserData>('list_voiceprints', { speakerId: null, unconfirmedOnly: false, limit: null, offset: null });
      setData(browser);
      const s = await invoke<StorageStats>('speaker_storage_stats');
      setStats(s);
      const list = await invoke<SpeakerLite[]>('list_speakers');
      setSpeakersList(list.map((x) => ({ id: x.id, name: x.name })));
      // Default state is collapsed: leave `expanded` as the empty set so all
      // speaker/meeting groups start collapsed. Expand-all / collapse-all and
      // per-group toggles remain available in the header.
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    if (audioPath && pendingRange && player.duration > 0) {
      player.playRange(pendingRange.start, pendingRange.end);
      setPendingRange(null);
    }
  }, [audioPath, pendingRange, player]);

  const toggleGroup = useCallback((key: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const expandAll = useCallback(() => {
    if (!data) return;
    const all = new Set<string>();
    data.speakers.forEach((sp) => all.add(`speaker:${sp.speaker_id}`));
    data.unconfirmed.forEach((mg) => all.add(`meeting:${mg.meeting_id}`));
    setExpanded(all);
  }, [data]);

  const collapseAll = useCallback(() => {
    setExpanded(new Set());
  }, []);

  const handlePlay = async (row: VoiceprintRow) => {
    if (row.audio_start_time == null || row.audio_end_time == null || !row.meeting_id) return;
    try {
      const path = await invoke<string>('get_meeting_audio_path', { meetingId: row.meeting_id });
      if (!path) {
        setError('No audio file for meeting');
        return;
      }
      setPendingRange({ start: row.audio_start_time, end: row.audio_end_time });
      setAudioPath(path);
    } catch (e) {
      setError(String(e));
    }
  };

  const handleReject = async (row: VoiceprintRow, permanent: boolean) => {
    const confirmText = permanent ? 'Permanently delete this voiceprint?' : 'Demote this voiceprint to unconfirmed cache?';
    if (!window.confirm(confirmText)) return;
    try {
      const res = await invoke<{ speaker_id: string | null; remaining_prototypes: number }>('reject_voiceprint', { id: row.id, permanent });
      if (res.speaker_id && res.remaining_prototypes === 0) {
        window.alert(`Speaker now has no voiceprints and will not be auto-assigned until reconfirmed.`);
      }
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  // ——— Shared picker state for reconfirm & replace (reuse same dialog) ———
  const [picker, setPicker] = useState<
    | { mode: 'reconfirm'; rowId: string }
    | { mode: 'replace'; sourceId: string; sourceName: string }
    | null
  >(null);
  const [confirm, setConfirm] = useState<{
    sourceId: string;
    sourceName: string;
    targetId: string | null;
    targetName: string | null;
    preview: { affected_meetings: number; affected_clusters: number; affected_transcripts: number } | null;
  } | null>(null);

  const handleReconfirmPick = async (speakerId: string) => {
    if (!picker || picker.mode !== 'reconfirm') return;
    try {
      await invoke('reconfirm_voiceprint', { id: picker.rowId, speakerId });
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleReconfirmCreate = async (name: string) => {
    if (!picker || picker.mode !== 'reconfirm') return;
    try {
      const sp = await invoke<SpeakerLite>('find_or_create_speaker', { name });
      setSpeakersList((prev) => (prev.some((p) => p.id === sp.id) ? prev : [...prev, sp]));
      await invoke('reconfirm_voiceprint', { id: picker.rowId, speakerId: sp.id });
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleReplacePick = async (targetId: string | null, targetName: string | null) => {
    if (!picker || picker.mode !== 'replace') return;
    // Fetch preview counts before confirming (shows affected meetings/clusters/transcripts)
    let preview: { affected_meetings: number; affected_clusters: number; affected_transcripts: number } | null = null;
    try {
      preview = await invoke<{ affected_meetings: number; affected_clusters: number; affected_transcripts: number }>('preview_replace_speaker', { source: picker.sourceId });
    } catch {
      preview = null;
    }
    setConfirm({
      sourceId: picker.sourceId,
      sourceName: picker.sourceName,
      targetId,
      targetName,
      preview,
    });
  };

  const handleReplaceSelect = (id: string) => {
    const sp = speakersList.find((s) => s.id === id);
    handleReplacePick(id, sp?.name ?? id);
  };

  const handleReplaceCreate = async (name: string) => {
    try {
      const sp = await invoke<SpeakerLite>('find_or_create_speaker', { name });
      setSpeakersList((prev) => (prev.some((p) => p.id === sp.id) ? prev : [...prev, sp]));
      await handleReplacePick(sp.id, sp.name);
    } catch (e) {
      setError(String(e));
    }
  };

  const handleReplaceAnonymous = () => {
    handleReplacePick(null, null);
  };

  const executeReplace = async () => {
    if (!confirm) return;
    try {
      const res = await invoke<{ affected_meetings: number; affected_clusters: number; affected_transcripts: number }>('replace_speaker', {
        source: confirm.sourceId,
        target: confirm.targetId,
      });
      setConfirm(null);
      setPicker(null);
      window.alert(`Replaced. Affected meetings: ${res.affected_meetings}, clusters: ${res.affected_clusters}, transcripts: ${res.affected_transcripts}`);
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  if (error) return <div className="p-4 text-red-600">{error}</div>;
  if (!data) return <div className="p-4">Loading voiceprints…</div>;

  const allSpeakerIds = data.speakers.map((sp) => `speaker:${sp.speaker_id}`);
  const allMeetingIds = data.unconfirmed.map((mg) => `meeting:${mg.meeting_id}`);
  const allIds = [...allSpeakerIds, ...allMeetingIds];
  const isAllExpanded = allIds.length > 0 && allIds.every((k) => expanded.has(k));
  const isAllCollapsed = allIds.every((k) => !expanded.has(k));

  return (
    <div className="space-y-6">
      <audio ref={player.audioRef} style={{ display: 'none' }} />
      {stats && (
        <div className="bg-white p-4 rounded border">
          <h3 className="font-semibold mb-2">Storage</h3>
          <div className="text-sm text-gray-600 flex gap-4 flex-wrap">
            <span>Speakers: {stats.registry_count}</span>
            <span>Prototypes: {stats.prototype_count}</span>
            <span>Unconfirmed caches: {stats.cache_count}</span>
            <span>Storage size: {formatBytes(stats.total_bytes)}</span>
          </div>
        </div>
      )}

      <div className="bg-white p-4 rounded border">
        <div className="flex items-center justify-between mb-2">
          <h3 className="font-semibold">Confirmed speakers</h3>
          <div className="flex items-center gap-1">
            <button
              onClick={expandAll}
              disabled={isAllExpanded || allIds.length === 0}
              className="text-xs px-2 py-1 bg-gray-100 rounded disabled:opacity-40 flex items-center gap-1"
              title="Expand all speaker and meeting groups"
              aria-label="Expand all groups"
            >
              <ChevronsDown className="h-3 w-3" /> Expand all
            </button>
            <button
              onClick={collapseAll}
              disabled={isAllCollapsed || allIds.length === 0}
              className="text-xs px-2 py-1 bg-gray-100 rounded disabled:opacity-40 flex items-center gap-1"
              title="Collapse all speaker and meeting groups"
              aria-label="Collapse all groups"
            >
              <ChevronsUp className="h-3 w-3" /> Collapse all
            </button>
          </div>
        </div>
        {data.speakers.length === 0 ? (
          <div className="text-sm text-gray-500">No confirmed voiceprints</div>
        ) : (
          data.speakers.map((sp) => {
            const key = `speaker:${sp.speaker_id}`;
            const isExpanded = expanded.has(key);
            return (
              <div key={sp.speaker_id} className="mb-4 border-b pb-2">
                <div className="flex items-center justify-between">
                  <button
                    onClick={() => toggleGroup(key)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        toggleGroup(key);
                      }
                    }}
                    aria-expanded={isExpanded}
                    aria-controls={`section-${key}`}
                    className="flex items-center gap-2 font-medium text-left hover:bg-gray-50 px-1 py-0.5 rounded"
                  >
                    {isExpanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
                    <span>
                      {sp.speaker_name} {sp.is_me ? '(you)' : ''} — {sp.prototype_count} prototypes
                    </span>
                    {!isExpanded && <span className="text-xs text-gray-400">(collapsed)</span>}
                  </button>
                  <button onClick={() => setPicker({ mode: 'replace', sourceId: sp.speaker_id, sourceName: sp.speaker_name })} className="text-xs px-2 py-1 bg-amber-100 rounded">
                    Replace speaker across meetings
                  </button>
                </div>
                {isExpanded ? (
                  <table id={`section-${key}`} className="w-full text-xs mt-2">
                    <thead>
                      <tr className="text-gray-500">
                        <th className="text-left">Channel</th>
                        <th className="text-left">Duration</th>
                        <th className="text-left">Meeting / Cluster</th>
                        <th className="text-left">Time range</th>
                        <th className="text-left">Actions</th>
                      </tr>
                    </thead>
                    <tbody>
                      {sp.prototypes.map((row) => {
                        const hasProvenance = row.meeting_id != null && row.audio_start_time != null;
                        const disabledPlay = row.audio_start_time == null || row.audio_end_time == null;
                        return (
                          <tr key={row.id} className="border-t">
                            <td>{row.channel}</td>
                            <td>{row.duration_secs.toFixed(2)}s</td>
                            <td>
                              {hasProvenance ? `${row.meeting_title ?? 'deleted meeting'} / ${row.cluster_label ?? ''}` : <span className="italic text-gray-400">source unavailable</span>}
                            </td>
                            <td>{row.audio_start_time != null && row.audio_end_time != null ? `${row.audio_start_time.toFixed(1)}–${row.audio_end_time.toFixed(1)}s` : '—'}</td>
                            <td className="flex gap-1 py-1">
                              <button disabled={disabledPlay} onClick={() => handlePlay(row)} className={`px-2 py-0.5 rounded text-xs ${disabledPlay ? 'bg-gray-100 text-gray-400' : 'bg-blue-500 text-white'}`}>
                                Play clip
                              </button>
                              <button onClick={() => handleReject(row, false)} className="px-2 py-0.5 bg-gray-200 rounded text-xs">
                                Reject
                              </button>
                              <button onClick={() => handleReject(row, true)} className="px-2 py-0.5 bg-red-100 rounded text-xs">
                                Delete
                              </button>
                            </td>
                          </tr>
                        );
                      })}
                      {sp.prototypes.length === 0 && (
                        <tr>
                          <td colSpan={5} className="text-center text-gray-400 py-2">
                            No voiceprints — reconfirm from unconfirmed below
                          </td>
                        </tr>
                      )}
                    </tbody>
                  </table>
                ) : null}
              </div>
            );
          })
        )}
      </div>

      <div className="bg-white p-4 rounded border">
        <div className="flex items-center justify-between mb-2">
          <h3 className="font-semibold">Unconfirmed caches (per meeting)</h3>
          <div className="flex items-center gap-1">
            <button
              onClick={expandAll}
              disabled={isAllExpanded || allIds.length === 0}
              className="text-xs px-2 py-1 bg-gray-100 rounded disabled:opacity-40 flex items-center gap-1"
              aria-label="Expand all groups"
            >
              <ChevronsDown className="h-3 w-3" /> Expand all
            </button>
            <button
              onClick={collapseAll}
              disabled={isAllCollapsed || allIds.length === 0}
              className="text-xs px-2 py-1 bg-gray-100 rounded disabled:opacity-40 flex items-center gap-1"
              aria-label="Collapse all groups"
            >
              <ChevronsUp className="h-3 w-3" /> Collapse all
            </button>
          </div>
        </div>
        {data.unconfirmed.length === 0 ? (
          <div className="text-sm text-gray-500">No unconfirmed caches</div>
        ) : (
          data.unconfirmed.map((mg) => {
            const key = `meeting:${mg.meeting_id}`;
            const isExpanded = expanded.has(key);
            return (
              <div key={mg.meeting_id} className="mb-4 border-b pb-2">
                <button
                  onClick={() => toggleGroup(key)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      toggleGroup(key);
                    }
                  }}
                  aria-expanded={isExpanded}
                  aria-controls={`section-${key}`}
                  className="flex items-center gap-2 font-medium text-left hover:bg-gray-50 px-1 py-0.5 rounded w-full"
                >
                  {isExpanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
                  <span>
                    {mg.meeting_title} — {mg.caches.length} caches
                  </span>
                  {!isExpanded && <span className="text-xs text-gray-400">(collapsed)</span>}
                </button>
                {isExpanded ? (
                  <table id={`section-${key}`} className="w-full text-xs mt-2">
                    <thead>
                      <tr className="text-gray-500">
                        <th className="text-left">Channel</th>
                        <th className="text-left">Duration</th>
                        <th className="text-left">Cluster</th>
                        <th className="text-left">Time range</th>
                        <th className="text-left">Actions</th>
                      </tr>
                    </thead>
                    <tbody>
                      {mg.caches.map((row) => {
                        const disabledPlay = row.audio_start_time == null || row.audio_end_time == null;
                        return (
                          <tr key={row.id} className="border-t">
                            <td>{row.channel}</td>
                            <td>{row.duration_secs.toFixed(2)}s</td>
                            <td>{row.cluster_label}</td>
                            <td>{row.audio_start_time != null && row.audio_end_time != null ? `${row.audio_start_time.toFixed(1)}–${row.audio_end_time.toFixed(1)}s` : '—'}</td>
                            <td className="flex gap-1 py-1">
                              <button disabled={disabledPlay} onClick={() => handlePlay(row)} className={`px-2 py-0.5 rounded text-xs ${disabledPlay ? 'bg-gray-100 text-gray-400' : 'bg-blue-500 text-white'}`}>
                                Play clip
                              </button>
                              <button onClick={() => setPicker({ mode: 'reconfirm', rowId: row.id })} className="px-2 py-0.5 bg-green-100 rounded text-xs">
                                Reconfirm
                              </button>
                              <button onClick={() => handleReject(row, true)} className="px-2 py-0.5 bg-gray-200 rounded text-xs">
                                Reject
                              </button>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                ) : null}
              </div>
            );
          })
        )}
      </div>

      {/* Shared person picker for reconfirm and replace (same dialog) */}
      <PersonPickerDialog
        open={picker?.mode === 'reconfirm'}
        onClose={() => setPicker(null)}
        speakers={speakersList}
        title="Reconfirm to speaker"
        onSelect={handleReconfirmPick}
        onCreate={handleReconfirmCreate}
      />
      <PersonPickerDialog
        open={picker?.mode === 'replace'}
        onClose={() => setPicker(null)}
        speakers={speakersList}
        excludeId={picker?.mode === 'replace' ? picker.sourceId : undefined}
        allowAnonymous
        title={picker?.mode === 'replace' ? `Replace "${picker.sourceName}" with…` : 'Select speaker'}
        onSelect={handleReplaceSelect}
        onCreate={handleReplaceCreate}
        onAnonymous={handleReplaceAnonymous}
      />
      <ConfirmReplaceDialog
        open={!!confirm}
        onClose={() => setConfirm(null)}
        onConfirm={executeReplace}
        sourceName={confirm?.sourceName ?? ''}
        targetName={confirm?.targetName ?? null}
        preview={confirm?.preview ?? null}
      />
    </div>
  );
}
