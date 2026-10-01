export function ConfirmReplaceDialog({
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
