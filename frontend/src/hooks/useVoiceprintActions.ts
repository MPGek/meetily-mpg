import { useEffect, useState, useCallback } from 'react';
import {
  clearAllVoiceprints,
  findOrCreateSpeaker,
  getVoiceprintAudio,
  listSpeakers,
  listVoiceprints,
  previewReplaceSpeaker,
  purgeUnconfirmedCaches,
  reconfirmVoiceprint,
  rejectVoiceprint,
  replaceSpeaker,
  speakerStorageStats,
  verifyMeetingCaches,
  verifySpeaker,
  verifyVoiceprint,
  type MeetingVoiceprints,
  type SpeakerVoiceprints,
  type StorageStats,
  type VoiceprintBrowserData,
  type VoiceprintRow,
} from '@/lib/ipc/speakers';
import { getMeetingAudioPath } from '@/lib/ipc/meetings';
import { useAudioPlayer } from '@/hooks/useAudioPlayer';
import { visibleRows } from '@/lib/voiceprint-suspect';
import type { SpeakerLite } from '@/components/Voiceprints/dialogs/PersonPickerDialog';
import { formatBytes } from '@/components/Voiceprints/formatBytes';

/** State, `load()` refetch and every action handler behind `VoiceprintBrowser`. */
export function useVoiceprintActions() {
  const [data, setData] = useState<VoiceprintBrowserData | null>(null);
  const [stats, setStats] = useState<StorageStats | null>(null);
  const [audioPath, setAudioPath] = useState<string | null>(null);
  const [pendingRange, setPendingRange] = useState<{ start: number; end: number } | null>(null);
  const [pendingBlobPlay, setPendingBlobPlay] = useState(false);
  const [playingRowId, setPlayingRowId] = useState<string | null>(null);
  const [failedRowId, setFailedRowId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [speakersList, setSpeakersList] = useState<SpeakerLite[]>([]);
  const [clearAllOpen, setClearAllOpen] = useState(false);
  const [purgeCachesOpen, setPurgeCachesOpen] = useState(false);
  const player = useAudioPlayer(audioPath);

  // Collapse/expand state — set of expanded group ids: `speaker:${id}` / `meeting:${id}`
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  const load = useCallback(async () => {
    try {
      const browser = await listVoiceprints({ speakerId: null, unconfirmedOnly: false, limit: null, offset: null });
      setData(browser);
      const s = await speakerStorageStats();
      setStats(s);
      const list = await listSpeakers();
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

  // Stored-clip playback: the temp file IS the clip, play it directly.
  useEffect(() => {
    if (audioPath && pendingBlobPlay && player.duration > 0) {
      player.play();
      setPendingBlobPlay(false);
    }
  }, [audioPath, pendingBlobPlay, player]);

  useEffect(() => {
    if (player.endedCount > 0) {
      setPlayingRowId(null);
    }
  }, [player.endedCount]);

  useEffect(() => {
    if (player.error && playingRowId) {
      setFailedRowId(playingRowId);
      setPlayingRowId(null);
    }
  }, [player.error, playingRowId]);

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
    if (playingRowId === row.id && player.isPlaying) {
      player.pause();
      setPlayingRowId(null);
      if (failedRowId === row.id) setFailedRowId(null);
      return;
    }
    if (failedRowId) setFailedRowId(null);
    // Blob-first: stored clips play without the meeting file or timecodes.
    if (row.has_audio) {
      try {
        const clipPath = await getVoiceprintAudio({ id: row.id });
        if (clipPath) {
          setPlayingRowId(row.id);
          setPendingBlobPlay(true);
          setPendingRange(null);
          setAudioPath(clipPath);
          return;
        }
        // Blob missing despite the flag (e.g. temp write failed) — fall
        // through to the legacy meeting-file path below.
      } catch (e) {
        setError(String(e));
        setFailedRowId(row.id);
        return;
      }
    }
    if (row.audio_start_time == null || row.audio_end_time == null || !row.meeting_id) return;
    try {
      const path = await getMeetingAudioPath({ meetingId: row.meeting_id });
      if (!path) {
        setError('No audio file for meeting');
        setFailedRowId(row.id);
        return;
      }
      setPlayingRowId(row.id);
      setPendingBlobPlay(false);
      setPendingRange({ start: row.audio_start_time, end: row.audio_end_time });
      setAudioPath(path);
    } catch (e) {
      setError(String(e));
      setFailedRowId(row.id);
    }
  };

  const handleReject = async (row: VoiceprintRow, permanent: boolean) => {
    const confirmText = permanent ? 'Permanently delete this voiceprint?' : 'Demote this voiceprint to unconfirmed cache?';
    if (!window.confirm(confirmText)) return;
    try {
      const res = await rejectVoiceprint({ id: row.id, permanent });
      if (res.speaker_id && res.remaining_prototypes === 0) {
        window.alert(`Speaker now has no voiceprints and will not be auto-assigned until reconfirmed.`);
      }
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  // ——— Verification (voiceprint-verification) ———
  const [hideVerified, setHideVerified] = useState(false);
  const [suspectOnly, setSuspectOnly] = useState(false);

  const handleVerifyRow = async (row: VoiceprintRow) => {
    try {
      await verifyVoiceprint({ id: row.id });
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleVerifySpeaker = async (speakerId: string) => {
    try {
      await verifySpeaker({ speakerId });
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleVerifyMeeting = async (meetingId: string) => {
    try {
      await verifyMeetingCaches({ meetingId });
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  const visiblePrototypes = (sp: SpeakerVoiceprints) =>
    visibleRows(sp.prototypes, { hideVerified, suspectOnly });
  const visibleCaches = (mg: MeetingVoiceprints) =>
    hideVerified ? mg.caches.filter((r) => r.is_verified === 0) : mg.caches;

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
      await reconfirmVoiceprint({ id: picker.rowId, speakerId });
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleReconfirmCreate = async (name: string) => {
    if (!picker || picker.mode !== 'reconfirm') return;
    try {
      const sp = await findOrCreateSpeaker({ name });
      setSpeakersList((prev) => (prev.some((p) => p.id === sp.id) ? prev : [...prev, sp]));
      await reconfirmVoiceprint({ id: picker.rowId, speakerId: sp.id });
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
      preview = await previewReplaceSpeaker({ source: picker.sourceId });
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
      const sp = await findOrCreateSpeaker({ name });
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
      const res = await replaceSpeaker({
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

  const handleClearAllConfirm = async () => {
    try {
      const res = await clearAllVoiceprints();
      setClearAllOpen(false);
      player.pause();
      setPlayingRowId(null);
      setFailedRowId(null);
      setPendingRange(null);
      await load();
      window.alert(`Cleared. Deleted prototypes: ${res.deleted_prototypes}, caches: ${res.deleted_caches}`);
    } catch (e) {
      setError(String(e));
    }
  };

  const handlePurgeCachesConfirm = async () => {
    try {
      const res = await purgeUnconfirmedCaches();
      setPurgeCachesOpen(false);
      player.pause();
      setPlayingRowId(null);
      setFailedRowId(null);
      setPendingRange(null);
      await load();
      window.alert(`Removed ${res.deleted_caches} unconfirmed caches. Reclaimed ${formatBytes(res.deleted_embedding_bytes + res.deleted_clip_bytes)}.`);
    } catch (e) {
      setError(String(e));
    }
  };

  return {
    data,
    stats,
    error,
    speakersList,
    player,
    playingRowId,
    failedRowId,
    expanded,
    clearAllOpen,
    setClearAllOpen,
    purgeCachesOpen,
    setPurgeCachesOpen,
    hideVerified,
    setHideVerified,
    suspectOnly,
    setSuspectOnly,
    picker,
    setPicker,
    confirm,
    setConfirm,
    toggleGroup,
    expandAll,
    collapseAll,
    handlePlay,
    handleReject,
    handleVerifyRow,
    handleVerifySpeaker,
    handleVerifyMeeting,
    visiblePrototypes,
    visibleCaches,
    handleReconfirmPick,
    handleReconfirmCreate,
    handleReplaceSelect,
    handleReplaceCreate,
    handleReplaceAnonymous,
    executeReplace,
    handleClearAllConfirm,
    handlePurgeCachesConfirm,
  };
}
