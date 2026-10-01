/**
 * Storage Service
 *
 * Handles all meeting storage and retrieval Tauri backend calls (SQLite persistence).
 * Pure 1-to-1 wrapper - no error handling changes, exact same behavior as direct invoke calls.
 */

import { saveTranscript, type SaveMeetingResponse } from '@/lib/ipc/transcript';
import { getMeeting, getMeetings, type Meeting, type MeetingDetails } from '@/lib/ipc/meetings';
import { Transcript } from '@/types';

export type { SaveMeetingResponse } from '@/lib/ipc/transcript';
export type { Meeting } from '@/lib/ipc/meetings';

export interface SaveMeetingRequest {
  meetingTitle: string;
  transcripts: Transcript[];
  folderPath: string | null;
}

/**
 * Storage Service
 * Singleton service for managing meeting storage operations
 */
export class StorageService {
  /**
   * Save meeting transcript to SQLite database
   * @param meetingTitle - Title of the meeting
   * @param transcripts - Array of transcript segments
   * @param folderPath - Optional folder path for audio file
   * @returns Promise with { meeting_id: string }
   */
  async saveMeeting(
    meetingTitle: string,
    transcripts: Transcript[],
    folderPath: string | null
  ): Promise<SaveMeetingResponse> {
    return saveTranscript({
      meetingTitle,
      transcripts,
      folderPath,
    });
  }

  /**
   * Get meeting details by ID
   * @param meetingId - ID of the meeting to fetch
   * @returns Promise with meeting details
   */
  async getMeeting(meetingId: string): Promise<MeetingDetails> {
    return getMeeting({ meetingId });
  }

  /**
   * Get list of all meetings
   * @returns Promise with array of meetings
   */
  async getMeetings(): Promise<Meeting[]> {
    return getMeetings();
  }
}

// Export singleton instance
export const storageService = new StorageService();
