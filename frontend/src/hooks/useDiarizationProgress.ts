import { useState, useEffect, useCallback, useRef } from 'react';
import { recordingService } from '@/services/recordingService';

export interface DiarizationProgressState {
  status: string | null;
  progress: number;
  message: string;
  isProcessing: boolean;
}

interface UseDiarizationProgressOptions {
  meetingId: string | null;
  onComplete?: () => void;
  onError?: () => void;
}

export function useDiarizationProgress({
  meetingId,
  onComplete,
  onError,
}: UseDiarizationProgressOptions): DiarizationProgressState {
  const [state, setState] = useState<DiarizationProgressState>({
    status: null,
    progress: 0,
    message: '',
    isProcessing: false,
  });

  const handleComplete = useCallback(() => {
    setState((prev) => ({
      ...prev,
      status: 'complete',
      progress: 100,
      isProcessing: false,
      message: 'Speaker analysis complete',
    }));
    onComplete?.();
  }, [onComplete]);

  const handleError = useCallback(() => {
    setState((prev) => ({
      ...prev,
      status: 'failed',
      isProcessing: false,
      message: prev.message || 'Speaker analysis failed',
    }));
    onError?.();
  }, [onError]);

  const prevMeetingIdRef = useRef<string | null>(null);

  useEffect(() => {
    if (!meetingId) return;

    const hasChanged = meetingId !== prevMeetingIdRef.current;
    prevMeetingIdRef.current = meetingId;

    if (hasChanged) {
      setState({
        status: null,
        progress: 0,
        message: '',
        isProcessing: false,
      });
    }

    let unlistenFn: (() => void) | undefined;

    const setup = async () => {
      try {
        // Seed initial status from backend
        const initial = await recordingService.getDiarizationStatus(meetingId);
        if (initial.diarization_status === 'processing') {
          setState((prev) => ({
            ...prev,
            status: 'processing',
            isProcessing: true,
            message: 'Speaker analysis in progress...',
          }));
        }

        unlistenFn = await recordingService.onDiarizationProgress((payload) => {
          if (payload.meeting_id !== meetingId) return;

          const isProcessing = payload.status === 'processing' ||
            payload.status === 'loading' ||
            payload.status === 'decoding' ||
            payload.status === 'diarizing' ||
            payload.status === 'matching';

          setState({
            status: payload.status,
            progress: payload.progress,
            message: payload.message,
            isProcessing,
          });

          if (payload.status === 'complete') {
            handleComplete();
          } else if (payload.status === 'failed') {
            handleError();
          }
        });
      } catch (error) {
        console.error('Failed to setup diarization progress listener:', error);
      }
    };

    setup();

    return () => {
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [meetingId, handleComplete, handleError]);

  return state;
}
