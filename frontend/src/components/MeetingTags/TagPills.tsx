'use client';

import React from 'react';
import { MeetingTag, tagPillClass } from '@/lib/meeting-tags';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

export const SIDEBAR_TAG_CAP = 2;

interface TagPillsProps {
  tags: MeetingTag[];
  cap?: number;
}

/** Compact tag pills with `+M` overflow; nothing renders for zero tags. */
export const TagPills: React.FC<TagPillsProps> = ({ tags, cap = SIDEBAR_TAG_CAP }) => {
  if (!tags || tags.length === 0) return null;
  const visible = tags.slice(0, cap);
  const hidden = tags.slice(cap);
  const hiddenNames = hidden.map((t) => t.name).join(', ');
  return (
    <div className="mt-0.5 flex flex-wrap items-center gap-1">
      {visible.map((tag) => (
        <span
          key={tag.id}
          title={tag.name}
          className={`inline-flex max-w-[92px] items-center truncate rounded-full border px-1.5 py-px text-[10px] leading-4 ${tagPillClass(tag.color)}`}
        >
          <span className="truncate">{tag.name}</span>
        </span>
      ))}
      {hidden.length > 0 && (
        <Tooltip>
          <TooltipTrigger asChild>
            <span className="inline-flex items-center rounded-full bg-gray-100 px-1.5 py-px text-[10px] leading-4 text-gray-600">
              +{hidden.length}
            </span>
          </TooltipTrigger>
          <TooltipContent side="right">
            <p className="max-w-[200px] break-words">{hiddenNames}</p>
          </TooltipContent>
        </Tooltip>
      )}
    </div>
  );
};
