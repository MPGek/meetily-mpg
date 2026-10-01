import type { StorageStats } from '@/lib/ipc/speakers';

export function ConfirmClearAllDialog({
  open,
  onClose,
  onConfirm,
  stats,
}: {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  stats: StorageStats | null;
}) {
  if (!open) return null;
  const proto = stats?.prototype_count ?? 0;
  const caches = stats?.cache_count ?? 0;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/40" onClick={onClose} aria-hidden />
      <div role="dialog" aria-modal="true" aria-label="Confirm bulk delete" className="relative bg-white rounded-lg shadow-xl w-[420px] border p-4">
        <h4 className="font-semibold text-sm mb-2">Remove all voiceprints & caches?</h4>
        <p className="text-sm text-gray-600 mb-3">
          This will permanently delete <span className="font-medium">{proto}</span> prototypes and <span className="font-medium">{caches}</span> cached embeddings for all speakers/meetings and cannot be undone.
        </p>
        <div className="bg-red-50 border border-red-200 rounded p-3 text-xs mb-3 text-red-800">
          All enrolled voiceprints and unassigned caches will be removed. The speaker names themselves will be kept, but they will have no voiceprints until re-enrolled.
        </div>
        <div className="flex justify-end gap-2">
          <button className="text-xs px-3 py-1.5 bg-gray-100 rounded" onClick={onClose}>
            Cancel
          </button>
          <button className="text-xs px-3 py-1.5 bg-red-600 text-white rounded" onClick={onConfirm} aria-label="Confirm delete all">
            Confirm delete all
          </button>
        </div>
      </div>
    </div>
  );
}
