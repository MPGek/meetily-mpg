import React, { useEffect, useState, useMemo } from 'react';
import type { Speaker } from '@/lib/ipc/speakers';

export type SpeakerLite = Pick<Speaker, 'id' | 'name'> & Partial<Pick<Speaker, 'is_me'>>;

// ——— Shared person picker (same as speaker name change / assignment) ———
export function PersonPickerDialog({
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
