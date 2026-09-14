'use client';

import React, { useEffect, useMemo, useState } from 'react';
import { Check, Plus, Tag as TagIcon, X } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import {
  MeetingTag,
  MeetingTagWithUsage,
  tagPillClass,
} from '@/lib/meeting-tags';

interface PendingTagsPickerProps {
  pending: MeetingTag[];
  onToggle: (tag: MeetingTag) => void;
  onCreate: (name: string) => Promise<void>;
  onRemove: (tagId: string) => void;
  label: string;
}

/**
 * Editor for the pending tag set of the upcoming / in-progress recording
 * (change: tags-before-during-recording). Same interaction pattern as the
 * sidebar tag editor, but operates on the pending set instead of a meeting.
 */
export const PendingTagsPicker: React.FC<PendingTagsPickerProps> = ({
  pending,
  onToggle,
  onCreate,
  onRemove,
  label,
}) => {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [allTags, setAllTags] = useState<MeetingTagWithUsage[]>([]);
  const [busy, setBusy] = useState(false);

  const pendingIds = useMemo(() => new Set(pending.map((t) => t.id)), [pending]);

  useEffect(() => {
    if (!open) return;
    setQuery('');
    invoke<MeetingTagWithUsage[]>('list_tags')
      .then(setAllTags)
      .catch((e) => console.error('Failed to list tags:', e));
  }, [open ]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return allTags;
    return allTags.filter((t) => t.name.toLowerCase().includes(q));
  }, [allTags, query]);

  const exactMatch = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return false;
    return allTags.some((t) => t.name.toLowerCase() === q);
  }, [allTags, query]);

  const handleCreate = async () => {
    const name = query.trim();
    if (!name || busy) return;
    setBusy(true);
    try {
      await onCreate(name);
      setQuery('');
    } catch (e) {
      toast.error('Failed to create tag', { description: String(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex items-center gap-1.5 rounded-full bg-white px-3 py-1.5 shadow-lg">
      <span className="text-[11px] text-gray-500">{label}</span>
      <div className="flex max-w-[280px] flex-wrap items-center gap-1">
        {pending.map((tag) => (
          <span
            key={tag.id}
            className={`inline-flex items-center gap-0.5 rounded-full border px-1.5 py-px text-[10px] leading-4 ${tagPillClass(tag.color)}`}
          >
            <span className="max-w-[80px] truncate">{tag.name}</span>
            <button
              onClick={() => onRemove(tag.id)}
              className="rounded-full p-px hover:bg-black/10"
              aria-label={`Remove ${tag.name}`}
            >
              <X className="h-3 w-3" />
            </button>
          </span>
        ))}
      </div>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            className="rounded-full p-1.5 text-gray-500 hover:bg-gray-100 hover:text-gray-700"
            aria-label="Pick tags for this recording"
            title="Pick tags for this recording"
          >
            <TagIcon className="h-4 w-4" />
          </button>
        </PopoverTrigger>
        <PopoverContent align="center" sideOffset={8} className="w-60 p-2">
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleCreate();
            }}
            placeholder="Search or create tag..."
            className="mb-1 w-full rounded-md border border-gray-200 px-2 py-1 text-xs focus:border-blue-400 focus:outline-none"
            autoFocus
          />
          <div className="max-h-44 overflow-y-auto">
            {filtered.map((tag) => {
              const active = pendingIds.has(tag.id);
              return (
                <button
                  key={tag.id}
                  onClick={() => onToggle(tag)}
                  className="flex w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-left hover:bg-gray-50"
                >
                  <span
                    className={`h-3 w-3 shrink-0 rounded-full border ${tagPillClass(tag.color)}`}
                  />
                  <span className="min-w-0 flex-1 truncate text-xs">{tag.name}</span>
                  {active && <Check className="h-3.5 w-3.5 shrink-0 text-blue-600" />}
                </button>
              );
            })}
            {filtered.length === 0 && !query.trim() && (
              <p className="px-2 py-3 text-center text-xs text-gray-400">No tags yet</p>
            )}
          </div>
          {query.trim() && !exactMatch && (
            <button
              onClick={handleCreate}
              disabled={busy}
              className="mt-1 flex w-full items-center gap-1.5 rounded-md bg-blue-50 px-2 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100 disabled:opacity-50"
            >
              <Plus className="h-3.5 w-3.5" />
              <span className="truncate">Create &ldquo;{query.trim()}&rdquo;</span>
            </button>
          )}
        </PopoverContent>
      </Popover>
    </div>
  );
};
