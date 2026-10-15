'use client';

import React from 'react';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { TagEditorPopover } from './TagEditorPopover';
import { tagPillClass } from '@/lib/meeting-tags';

interface MeetingTagsBarProps {
  meetingId?: string;
}

/**
 * Full tag row for the meeting-details page. Reads tags from the shared
 * SidebarProvider meetings cache (no extra fetch loop) and reuses the same
 * popover editor as the sidebar rows.
 */
export const MeetingTagsBar: React.FC<MeetingTagsBarProps> = ({ meetingId }) => {
  const { meetings, refetchMeetings } = useSidebar();
  if (!meetingId) return null;
  const meeting = meetings.find((m) => m.id === meetingId);
  const tags = meeting?.tags ?? [];
  return (
    <div className="mt-2 flex flex-wrap items-center gap-1">
      {tags.map((tag) => (
        <span
          key={tag.id}
          title={tag.name}
          className={`inline-flex max-w-[140px] items-center truncate rounded-full border px-2 py-0.5 text-[11px] leading-4 ${tagPillClass(tag.color)}`}
        >
          <span className="truncate">{tag.name}</span>
        </span>
      ))}
      <TagEditorPopover
        meetingId={meetingId}
        assigned={tags}
        onChanged={() => refetchMeetings()}
      />
    </div>
  );
};
