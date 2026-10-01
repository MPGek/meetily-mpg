import { useCallback } from 'react';
import { openMeetingFolder } from '@/lib/ipc/meetings';
import { toast } from 'sonner';

interface UseMeetingOperationsProps {
  meeting: any;
}

export function useMeetingOperations({
  meeting,
}: UseMeetingOperationsProps) {

  // Open meeting folder in file explorer
  const handleOpenMeetingFolder = useCallback(async () => {
    try {
      await openMeetingFolder({ meetingId: meeting.id });
    } catch (error) {
      console.error('Failed to open meeting folder:', error);
      toast.error(String(error) || 'Failed to open recording folder');
    }
  }, [meeting.id]);

  return {
    handleOpenMeetingFolder,
  };
}
