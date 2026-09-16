'use client';

import React, { useEffect, useMemo, useState } from 'react';
import { Check, Plus, Tag as TagIcon, Trash2, X } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import {
  MeetingTag,
  MeetingTagWithUsage,
  nextColor,
  tagPillClass,
} from '@/lib/meeting-tags';

interface TagEditorPopoverProps {
  meetingId: string;
  assigned: MeetingTag[];
  onChanged: () => void;
}

/** Tag picker for one meeting: pick existing, create new, remove, delete. */
export const TagEditorPopover: React.FC<TagEditorPopoverProps> = ({
  meetingId,
  assigned,
  onChanged,
}) => {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [allTags, setAllTags] = useState<MeetingTagWithUsage[]>([]);
  const [busy, setBusy] = useState(false);

  const assignedIds = useMemo(() => new Set(assigned.map((t) => t.id)), [assigned]);

  const load = async () => {
    try {
      const tags = await invoke<MeetingTagWithUsage[]>('list_tags');
      setAllTags(tags);
    } catch (e) {
      console.error('Failed to list tags:', e);
    }
  };

  useEffect(() => {
    if (open) {
      setQuery('');
      load();
    }
  }, [open]);

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

  const refresh = async () => {
    await load();
    onChanged();
  };

  const toggle = async (tag: MeetingTagWithUsage) => {
    if (busy) return;
    setBusy(true);
    try {
      if (assignedIds.has(tag.id)) {
        await invoke('unassign_tag', { meetingId, tagId: tag.id });
      } else {
        await invoke('assign_tag', { meetingId, tagId: tag.id });
      }
      await refresh();
    } catch (e) {
      toast.error('Failed to update tag', { description: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const createAndAssign = async () => {
    const name = query.trim();
    if (!name || busy) return;
    setBusy(true);
    try {
      await invoke('create_and_assign_tag', { meetingId, name });
      setQuery('');
      await refresh();
    } catch (e) {
      toast.error('Failed to create tag', { description: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const removeAssigned = async (tag: MeetingTag) => {
    try {
      await invoke('unassign_tag', { meetingId, tagId: tag.id });
      await refresh();
    } catch (e) {
      toast.error('Failed to remove tag', { description: String(e) });
    }
  };

  const deleteTag = async (tag: MeetingTagWithUsage) => {
    try {
      await invoke('delete_tag', { tagId: tag.id });
      toast.success(`Tag "${tag.name}" deleted`);
      await refresh();
    } catch (e) {
      toast.error('Failed to delete tag', { description: String(e) });
    }
  };

  const cycleColor = async (tag: MeetingTagWithUsage) => {
    try {
      await invoke('set_tag_color', { tagId: tag.id, color: nextColor(tag.color) });
      await refresh();
    } catch (e) {
      toast.error('Failed to change color', { description: String(e) });
    }
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          onClick={(e) => {
            e.stopPropagation();
            setOpen(true);
          }}
          className="rounded-md p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600"
          aria-label="Edit tags"
          title="Edit tags"
        >
          <TagIcon className="h-3.5 w-3.5" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        sideOffset={6}
        className="w-60 p-2"
        onClick={(e) => e.stopPropagation()}
      >
        {assigned.length > 0 && (
          <div className="mb-2 flex flex-wrap gap-1 border-b border-gray-100 pb-2">
            {assigned.map((tag) => (
              <span
                key={tag.id}
                className={`inline-flex items-center gap-0.5 rounded-full border px-1.5 py-px text-[10px] leading-4 ${tagPillClass(tag.color)}`}
              >
                <span className="max-w-[80px] truncate">{tag.name}</span>
                <button
                  onClick={() => removeAssigned(tag)}
                  className="rounded-full p-px hover:bg-black/10"
                  aria-label={`Remove ${tag.name}`}
                >
                  <X className="h-3 w-3" />
                </button>
              </span>
            ))}
          </div>
        )}
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') createAndAssign();
          }}
          placeholder="Search or create tag..."
          className="mb-1 w-full rounded-md border border-gray-200 px-2 py-1 text-xs focus:border-blue-400 focus:outline-none"
          autoFocus
        />
        <div className="max-h-44 overflow-y-auto">
          {filtered.map((tag) => {
            const active = assignedIds.has(tag.id);
            return (
              <div
                key={tag.id}
                className="group flex items-center gap-1.5 rounded-md px-1.5 py-1 hover:bg-gray-50"
              >
                <button
                  onClick={() => cycleColor(tag)}
                  title={`Color: ${tag.color} (click to change)`}
                  className={`h-3 w-3 shrink-0 rounded-full border ${tagPillClass(tag.color)}`}
                />
                <button
                  onClick={() => toggle(tag)}
                  className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
                >
                  <span className="min-w-0 flex-1 truncate text-xs">{tag.name}</span>
                  {tag.usage_count > 0 && (
                    <span className="text-[10px] text-gray-400">{tag.usage_count}</span>
                  )}
                  {active && <Check className="h-3.5 w-3.5 shrink-0 text-blue-600" />}
                </button>
                <button
                  onClick={() => deleteTag(tag)}
                  className="rounded p-0.5 text-gray-300 opacity-0 hover:bg-red-50 hover:text-red-600 group-hover:opacity-100"
                  aria-label={`Delete ${tag.name}`}
                  title={`Delete ${tag.name}`}
                >
                  <Trash2 className="h-3 w-3" />
                </button>
              </div>
            );
          })}
          {filtered.length === 0 && !query.trim() && (
            <p className="px-2 py-3 text-center text-xs text-gray-400">No tags yet</p>
          )}
        </div>
        {query.trim() && !exactMatch && (
          <button
            onClick={createAndAssign}
            disabled={busy}
            className="mt-1 flex w-full items-center gap-1.5 rounded-md bg-blue-50 px-2 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100 disabled:opacity-50"
          >
            <Plus className="h-3.5 w-3.5" />
            <span className="truncate">Create &ldquo;{query.trim()}&rdquo;</span>
          </button>
        )}
      </PopoverContent>
    </Popover>
  );
};
