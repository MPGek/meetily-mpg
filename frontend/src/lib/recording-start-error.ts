import { normalizeMessage } from '@/lib/ipc/core';

/** Alert text for a failed recording start, carrying the backend's reason. */
export function formatRecordingStartError(error: unknown): string {
  const message = normalizeMessage(error).trim();
  return message ? `Failed to start recording.\n\n${message}` : 'Failed to start recording.';
}
