"use client";

import { useState, useCallback, useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import { Copy, FolderOpen, RefreshCw, Users } from 'lucide-react';
import Analytics from '@/lib/analytics';
import { RetranscribeDialog } from './RetranscribeDialog';
import { useConfig } from '@/contexts/ConfigContext';
import { recordingService } from '@/services/recordingService';
import { loadDiarizationSettings } from '@/lib/diarization';
import { useRouter } from 'next/navigation';
import { toast } from 'sonner';


interface TranscriptButtonGroupProps {
  transcriptCount: number;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => Promise<void>;
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
}


export function TranscriptButtonGroup({
  transcriptCount,
  onCopyTranscript,
  onOpenMeetingFolder,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
}: TranscriptButtonGroupProps) {
  const { betaFeatures } = useConfig();
  const router = useRouter();
  const [showRetranscribeDialog, setShowRetranscribeDialog] = useState(false);
  const [isDiarizing, setIsDiarizing] = useState(false);
  const [modelsReady, setModelsReady] = useState(false);

  useEffect(() => {
    let cancelled = false;
    recordingService.checkDiarizationModels()
      .then((status) => {
        if (!cancelled) {
          setModelsReady(status.segmentation_ready && status.embedding_ready);
        }
      })
      .catch((error) => {
        console.error('Failed to check diarization models:', error);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleRetranscribeComplete = useCallback(async () => {
    if (onRefetchTranscripts) {
      await onRefetchTranscripts();
    }
  }, [onRefetchTranscripts]);

  const openSettings = () => {
    router.push('/settings?tab=general');
  };

  const handleReanalyzeSpeakers = async () => {
    if (!meetingId || isDiarizing) return;
    setIsDiarizing(true);
    try {
      Analytics.trackButtonClick('reanalyze_speakers', 'meeting_details');
      const settings = loadDiarizationSettings();
      const maxSpeakers = settings.maxSpeakers > 0 ? settings.maxSpeakers : undefined;
      await recordingService.startDiarization(
        meetingId,
        maxSpeakers
      );
      // Refetch handled by useDiarizationProgress onComplete event
    } catch (err: any) {
      console.error('Diarization failed:', err);
      const message = err instanceof Error ? err.message : String(err);
      const isModelMissing = /model.*not found|download models/i.test(message);
      toast.error('Speaker analysis failed', {
        description: message,
        action: isModelMissing
          ? {
              label: 'Open Settings',
              onClick: openSettings,
            }
          : undefined,
      });
    } finally {
      setIsDiarizing(false);
    }
  };

  return (
    <div className="flex items-center justify-center w-full gap-2">
      <ButtonGroup>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            Analytics.trackButtonClick('copy_transcript', 'meeting_details');
            onCopyTranscript();
          }}
          disabled={transcriptCount === 0}
          title={transcriptCount === 0 ? 'No transcript available' : 'Copy Transcript'}
        >
          <Copy />
          <span className="hidden lg:inline">Copy</span>
        </Button>

        <Button
          size="sm"
          variant="outline"
          className="xl:px-4"
          onClick={() => {
            Analytics.trackButtonClick('open_recording_folder', 'meeting_details');
            onOpenMeetingFolder();
          }}
          title="Open Recording Folder"
        >
          <FolderOpen className="xl:mr-2" size={18} />
          <span className="hidden lg:inline">Recording</span>
        </Button>

        {betaFeatures.importAndRetranscribe && meetingId && meetingFolderPath && (
          <Button
            size="sm"
            variant="outline"
            className="bg-gradient-to-r from-blue-50 to-purple-50 hover:from-blue-100 hover:to-purple-100 border-blue-200 xl:px-4"
            onClick={() => {
              Analytics.trackButtonClick('enhance_transcript', 'meeting_details');
              setShowRetranscribeDialog(true);
            }}
            title="Retranscribe to enhance your recorded audio"
          >
            <RefreshCw className="xl:mr-2" size={18} />
            <span className="hidden lg:inline">Enhance</span>
          </Button>
        )}

        {meetingId && modelsReady && (
          <Button
            size="sm"
            variant="outline"
            onClick={handleReanalyzeSpeakers}
            disabled={isDiarizing}
            title="Identify who spoke when"
          >
            {isDiarizing ? (
              <RefreshCw className="xl:mr-2 animate-spin" size={18} />
            ) : (
              <Users className="xl:mr-2" size={18} />
            )}
            <span className="hidden lg:inline">{isDiarizing ? 'Analyzing...' : 'Speakers'}</span>
          </Button>
        )}

        {meetingId && !modelsReady && (
          <Button
            size="sm"
            variant="outline"
            onClick={openSettings}
            title="Download diarization models in settings"
          >
            <Users className="xl:mr-2" size={18} />
            <span className="hidden lg:inline">Setup Speakers</span>
          </Button>
        )}
      </ButtonGroup>

      {betaFeatures.importAndRetranscribe && meetingId && meetingFolderPath && (
        <RetranscribeDialog
          open={showRetranscribeDialog}
          onOpenChange={setShowRetranscribeDialog}
          meetingId={meetingId}
          meetingFolderPath={meetingFolderPath}
          onComplete={handleRetranscribeComplete}
        />
      )}
    </div>
  );
}
