'use client';

import { useVoiceprintActions } from '@/hooks/useVoiceprintActions';
import { suspectBadge, hasSuspect } from '@/lib/voiceprint-suspect';
import { ChevronDown, ChevronRight, ChevronsDown, ChevronsUp, Play, Pause, AlertCircle, Trash2 } from 'lucide-react';
import { formatBytes } from '@/components/Voiceprints/formatBytes';
import { PersonPickerDialog } from '@/components/Voiceprints/dialogs/PersonPickerDialog';
import { ConfirmReplaceDialog } from '@/components/Voiceprints/dialogs/ConfirmReplaceDialog';
import { ConfirmClearAllDialog } from '@/components/Voiceprints/dialogs/ConfirmClearAllDialog';
import { ConfirmPurgeCachesDialog } from '@/components/Voiceprints/dialogs/ConfirmPurgeCachesDialog';

export default function VoiceprintBrowser() {
  const {
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
  } = useVoiceprintActions();

  if (error) return <div className="p-4 text-red-600">{error}</div>;
  if (!data) return <div className="p-4">Loading voiceprints…</div>;

  const allSpeakerIds = data.speakers.map((sp) => `speaker:${sp.speaker_id}`);
  const allMeetingIds = data.unconfirmed.map((mg) => `meeting:${mg.meeting_id}`);
  const allIds = [...allSpeakerIds, ...allMeetingIds];
  // Cache rows carrying a stored clip — the clips a purge would remove.
  const cacheClipCount = data.unconfirmed.reduce(
    (n, mg) => n + mg.caches.filter((c) => c.has_audio).length,
    0
  );
  const isAllExpanded = allIds.length > 0 && allIds.every((k) => expanded.has(k));
  const isAllCollapsed = allIds.every((k) => !expanded.has(k));

  return (
    <div className="space-y-6">
      <audio ref={player.audioRef} style={{ display: 'none' }} />
      {stats && (
        <div className="bg-white p-4 rounded border">
          <div className="flex items-center justify-between mb-2">
            <h3 className="font-semibold">Storage</h3>
            <div className="flex items-center gap-1">
              <button
                onClick={() => setPurgeCachesOpen(true)}
                disabled={stats.cache_count === 0}
                className="text-xs px-2 py-1 bg-amber-100 text-amber-800 rounded disabled:opacity-40 flex items-center gap-1 hover:bg-amber-200"
                title="Remove unconfirmed caches only, keeping confirmed speaker voiceprints"
                aria-label="Remove unconfirmed caches only"
              >
                <Trash2 className="h-3 w-3" /> Remove unconfirmed caches
              </button>
              <button
                onClick={() => setClearAllOpen(true)}
                disabled={stats.prototype_count + stats.cache_count === 0}
                className="text-xs px-2 py-1 bg-red-100 text-red-700 rounded disabled:opacity-40 flex items-center gap-1 hover:bg-red-200"
                title="Remove all voiceprints and cached embeddings"
                aria-label="Remove all voiceprints and cached embeddings"
              >
                <Trash2 className="h-3 w-3" /> Remove all
              </button>
            </div>
          </div>
          <div className="text-sm text-gray-600 flex gap-4 flex-wrap">
            <span>Speakers: {stats.registry_count}</span>
            <span>Prototypes: {stats.prototype_count}</span>
            <span>Unconfirmed caches: {stats.cache_count}</span>
            <span>Embeddings: {formatBytes(stats.total_bytes)}</span>
            <span>Voice clips: {formatBytes(stats.audio_bytes)} ({stats.clip_count} clips)</span>
          </div>
        </div>
      )}

      <div className="bg-white p-4 rounded border">
        <div className="flex items-center justify-between mb-2">
          <h3 className="font-semibold">Confirmed speakers</h3>
          <div className="flex items-center gap-1">
            <label className="text-xs px-2 py-1 bg-gray-100 rounded flex items-center gap-1 cursor-pointer" title="Show only unverified voiceprints">
              <input
                type="checkbox"
                checked={hideVerified}
                onChange={(e) => setHideVerified(e.target.checked)}
                aria-label="Hide verified voiceprints"
              />
              Hide verified
            </label>
            <label className="text-xs px-2 py-1 bg-gray-100 rounded flex items-center gap-1 cursor-pointer" title="Show only prototypes that look foreign to their speaker">
              <input
                type="checkbox"
                checked={suspectOnly}
                onChange={(e) => setSuspectOnly(e.target.checked)}
                aria-label="Show only suspect voiceprints"
              />
              Suspect only
            </label>
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
          data.speakers.filter((sp) => !suspectOnly || hasSuspect(sp.prototypes)).map((sp) => {
            const key = `speaker:${sp.speaker_id}`;
            const isExpanded = expanded.has(key);
            const rows = visiblePrototypes(sp);
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
                    {sp.unverified_count > 0 && (
                      <span className="text-xs px-1.5 py-0.5 bg-amber-100 text-amber-800 rounded" title="Unverified voiceprints">
                        {sp.unverified_count} new
                      </span>
                    )}
                    {(sp.suspect_count ?? 0) > 0 && (
                      <span className="text-xs px-1.5 py-0.5 bg-red-100 text-red-700 rounded" title="Prototypes that look foreign to this speaker">
                        {sp.suspect_count} suspect
                      </span>
                    )}
                    {!isExpanded && <span className="text-xs text-gray-400">(collapsed)</span>}
                  </button>
                  <div className="flex gap-1">
                    <button
                      onClick={() => handleVerifySpeaker(sp.speaker_id)}
                      disabled={sp.unverified_count === 0}
                      className="text-xs px-2 py-1 bg-green-100 rounded disabled:opacity-40"
                      title="Mark all prototypes of this speaker as verified"
                    >
                      Verify all
                    </button>
                    <button onClick={() => setPicker({ mode: 'replace', sourceId: sp.speaker_id, sourceName: sp.speaker_name })} className="text-xs px-2 py-1 bg-amber-100 rounded">
                      Replace speaker across meetings
                    </button>
                  </div>
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
                      {rows.map((row) => {
                        const hasProvenance = row.meeting_id != null && row.audio_start_time != null;
                        const disabledPlay = !row.has_audio && (row.audio_start_time == null || row.audio_end_time == null);
                        const isPlayingRow = playingRowId === row.id && player.isPlaying;
                        const isFailedRow = failedRowId === row.id;
                        const verified = row.is_verified !== 0;
                        return (
                          <tr key={row.id} className="border-t">
                            <td>{row.channel}</td>
                            <td>{row.duration_secs.toFixed(2)}s</td>
                            <td>
                              {hasProvenance ? `${row.meeting_title ?? 'deleted meeting'} / ${row.cluster_label ?? ''}` : <span className="italic text-gray-400">source unavailable</span>}
                              {!row.has_audio && <span className="italic text-gray-400"> · no clip</span>}
                            </td>
                            <td>{row.audio_start_time != null && row.audio_end_time != null ? `${row.audio_start_time.toFixed(1)}–${row.audio_end_time.toFixed(1)}s` : '—'}</td>
                            <td className="flex gap-1 py-1 flex-wrap items-center">
                              {suspectBadge(row) && (
                                <span
                                  className={`px-1.5 py-0.5 text-xs rounded ${suspectBadge(row) === 'suspect' ? 'bg-red-100 text-red-700' : 'bg-gray-100 text-gray-500'}`}
                                  title={`Sounds unlike this speaker's other voiceprints${row.own_similarity != null ? ` (similarity ${row.own_similarity.toFixed(2)})` : ''}`}
                                >
                                  {suspectBadge(row) === 'suspect' ? 'suspect' : 'suspect · checked'}
                                </span>
                              )}
                              <button disabled={disabledPlay} onClick={() => handlePlay(row)} className={`px-2 py-0.5 rounded text-xs flex items-center gap-1 ${disabledPlay ? 'bg-gray-100 text-gray-400' : isFailedRow ? 'bg-red-600 text-white' : isPlayingRow ? 'bg-blue-700 text-white' : 'bg-blue-500 text-white'}`}>
                                {isFailedRow ? <AlertCircle className="h-3 w-3" /> : isPlayingRow ? <Pause className="h-3 w-3" /> : <Play className="h-3 w-3" />}
                                Play clip
                              </button>
                              {verified ? (
                                <span className="px-2 py-0.5 text-xs text-green-700" title="Voice confirmed correct">✓ verified</span>
                              ) : (
                                <button onClick={() => handleVerifyRow(row)} className="px-2 py-0.5 bg-green-100 rounded text-xs" title="Confirm this voice is correct">
                                  Verify
                                </button>
                              )}
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
                      {rows.length === 0 && (
                        <tr>
                          <td colSpan={5} className="text-center text-gray-400 py-2">
                            {hideVerified ? 'All voiceprints verified — nothing new to review' : 'No voiceprints — reconfirm from unconfirmed below'}
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
            const rows = visibleCaches(mg);
            return (
              <div key={mg.meeting_id} className="mb-4 border-b pb-2">
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
                      {mg.meeting_title} — {mg.caches.length} caches
                    </span>
                    {mg.unverified_count > 0 && (
                      <span className="text-xs px-1.5 py-0.5 bg-amber-100 text-amber-800 rounded" title="Unverified caches">
                        {mg.unverified_count} new
                      </span>
                    )}
                    {!isExpanded && <span className="text-xs text-gray-400">(collapsed)</span>}
                  </button>
                  <button
                    onClick={() => handleVerifyMeeting(mg.meeting_id)}
                    disabled={mg.unverified_count === 0}
                    className="text-xs px-2 py-1 bg-green-100 rounded disabled:opacity-40"
                    title="Mark all caches of this meeting as verified"
                  >
                    Verify all
                  </button>
                </div>
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
                      {rows.map((row) => {
                        const disabledPlay = !row.has_audio && (row.audio_start_time == null || row.audio_end_time == null);
                        const isPlayingRow = playingRowId === row.id && player.isPlaying;
                        const isFailedRow = failedRowId === row.id;
                        const verified = row.is_verified !== 0;
                        return (
                          <tr key={row.id} className="border-t">
                            <td>{row.channel}</td>
                            <td>{row.duration_secs.toFixed(2)}s</td>
                            <td>{row.cluster_label}{!row.has_audio && <span className="italic text-gray-400"> · no clip</span>}</td>
                            <td>{row.audio_start_time != null && row.audio_end_time != null ? `${row.audio_start_time.toFixed(1)}–${row.audio_end_time.toFixed(1)}s` : '—'}</td>
                            <td className="flex gap-1 py-1 flex-wrap items-center">
                              <button disabled={disabledPlay} onClick={() => handlePlay(row)} className={`px-2 py-0.5 rounded text-xs flex items-center gap-1 ${disabledPlay ? 'bg-gray-100 text-gray-400' : isFailedRow ? 'bg-red-600 text-white' : isPlayingRow ? 'bg-blue-700 text-white' : 'bg-blue-500 text-white'}`}>
                                {isFailedRow ? <AlertCircle className="h-3 w-3" /> : isPlayingRow ? <Pause className="h-3 w-3" /> : <Play className="h-3 w-3" />}
                                Play clip
                              </button>
                              {verified ? (
                                <span className="px-2 py-0.5 text-xs text-green-700" title="Voice confirmed correct">✓ verified</span>
                              ) : (
                                <button onClick={() => handleVerifyRow(row)} className="px-2 py-0.5 bg-green-100 rounded text-xs" title="Confirm this voice is correct">
                                  Verify
                                </button>
                              )}
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
                      {rows.length === 0 && (
                        <tr>
                          <td colSpan={5} className="text-center text-gray-400 py-2">
                            {hideVerified ? 'All caches verified — nothing new to review' : 'No caches'}
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
      <ConfirmClearAllDialog
        open={clearAllOpen}
        onClose={() => setClearAllOpen(false)}
        onConfirm={handleClearAllConfirm}
        stats={stats}
      />
      <ConfirmPurgeCachesDialog
        open={purgeCachesOpen}
        onClose={() => setPurgeCachesOpen(false)}
        onConfirm={handlePurgeCachesConfirm}
        cacheCount={stats?.cache_count ?? 0}
        clipCount={cacheClipCount}
      />
    </div>
  );
}
