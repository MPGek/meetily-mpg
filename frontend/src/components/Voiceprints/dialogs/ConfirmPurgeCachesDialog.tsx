export function ConfirmPurgeCachesDialog({
  open,
  onClose,
  onConfirm,
  cacheCount,
  clipCount,
}: {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  cacheCount: number;
  clipCount: number;
}) {
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/40" onClick={onClose} aria-hidden />
      <div role="dialog" aria-modal="true" aria-label="Confirm unconfirmed cache purge" className="relative bg-white rounded-lg shadow-xl w-[420px] border p-4">
        <h4 className="font-semibold text-sm mb-2">Remove unconfirmed caches?</h4>
        <p className="text-sm text-gray-600 mb-3">
          This will permanently delete <span className="font-medium">{cacheCount}</span> unconfirmed cache embeddings
          {clipCount > 0 && (
            <>
              {' '}and <span className="font-medium">{clipCount}</span> stored voice{' '}
              {clipCount === 1 ? 'clip' : 'clips'}
            </>
          )}
          , and cannot be undone.
        </p>
        <div className="bg-amber-50 border border-amber-200 rounded p-3 text-xs mb-3 text-amber-900">
          Confirmed speaker prototypes are kept, so recognition and existing speaker names are unaffected. Caches are the
          source used when naming a cluster, so they can be recreated only by running diarization again for the affected
          meetings.
        </div>
        <div className="flex justify-end gap-2">
          <button className="text-xs px-3 py-1.5 bg-gray-100 rounded" onClick={onClose}>
            Cancel
          </button>
          <button className="text-xs px-3 py-1.5 bg-red-600 text-white rounded" onClick={onConfirm} aria-label="Confirm purge unconfirmed caches">
            Confirm purge
          </button>
        </div>
      </div>
    </div>
  );
}
