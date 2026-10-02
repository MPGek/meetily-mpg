'use client';

import React, { createContext, useContext, useState, useEffect, useRef } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import Analytics from '@/lib/analytics';
import { getMeetings, searchTranscripts as searchTranscriptsIpc, type TranscriptSearchResult } from '@/lib/ipc/meetings';
import { getSummary } from '@/lib/ipc/summary';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import type { MeetingTag } from '@/lib/meeting-tags';


interface SidebarItem {
  id: string;
  title: string;
  type: 'folder' | 'file';
  created_at?: string;
  started_at?: string | null;
  tags?: MeetingTag[];
  children?: SidebarItem[];
}

export interface CurrentMeeting {
  id: string;
  title: string;
  created_at?: string;
  started_at?: string | null;
  tags?: MeetingTag[];
}

interface SidebarContextType {
  currentMeeting: CurrentMeeting | null;
  setCurrentMeeting: (meeting: CurrentMeeting | null) => void;
  sidebarItems: SidebarItem[];
  isCollapsed: boolean;
  toggleCollapse: () => void;
  meetings: CurrentMeeting[];
  setMeetings: (meetings: CurrentMeeting[]) => void;
  isMeetingActive: boolean;
  setIsMeetingActive: (active: boolean) => void;
  handleRecordingToggle: () => void;
  searchTranscripts: (query: string) => Promise<void>;
  searchResults: TranscriptSearchResult[];
  isSearching: boolean;
  setServerAddress: (address: string) => void;
  serverAddress: string;
  transcriptServerAddress: string;
  setTranscriptServerAddress: (address: string) => void;
  // Summary polling management
  startSummaryPolling: (meetingId: string, processId: string, onUpdate: (result: any) => void | Promise<void>) => void;
  /** Stops the meeting's poll; with `processId`, only if that run is the one being polled. */
  stopSummaryPolling: (meetingId: string, processId?: string) => void;
  // Refetch meetings from backend
  refetchMeetings: () => Promise<void>;

}

/** One meeting's summary poll: the run it tracks, its timer, and whether a read is in flight. */
interface SummaryPoll {
  processId: string;
  timer: ReturnType<typeof setInterval>;
  inFlight: boolean;
}

const SidebarContext = createContext<SidebarContextType | null>(null);

export const useSidebar = () => {
  const context = useContext(SidebarContext);
  if (!context) {
    throw new Error('useSidebar must be used within a SidebarProvider');
  }
  return context;
};

export function SidebarProvider({ children }: { children: React.ReactNode }) {
  const [currentMeeting, setCurrentMeeting] = useState<CurrentMeeting | null>({ id: 'intro-call', title: '+ New Call' });
  const [isCollapsed, setIsCollapsed] = useState(true);
  const [meetings, setMeetings] = useState<CurrentMeeting[]>([]);
  const [sidebarItems, setSidebarItems] = useState<SidebarItem[]>([]);
  const [isMeetingActive, setIsMeetingActive] = useState(false);
  const [searchResults, setSearchResults] = useState<any[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [serverAddress, setServerAddress] = useState('');
  const [transcriptServerAddress, setTranscriptServerAddress] = useState('');
  // One summary poll per meeting. Held in a ref so starting or stopping one
  // poll never re-creates the callbacks or clears another meeting's poll.
  const summaryPollsRef = useRef(new Map<string, SummaryPoll>());

  // Use recording state from RecordingStateContext (single source of truth)
  const { isRecording } = useRecordingState();

  const pathname = usePathname();
  const router = useRouter();

  // Extract fetchMeetings as a reusable function
  const fetchMeetings = React.useCallback(async () => {
    if (serverAddress) {
      try {
        const meetings = await getMeetings();
        const transformedMeetings = meetings.map((meeting: any) => ({
          id: meeting.id,
          title: meeting.title,
          created_at: meeting.created_at,
          started_at: meeting.started_at ?? null,
          tags: meeting.tags ?? [],
        }));
        setMeetings(transformedMeetings);
        Analytics.trackBackendConnection(true);
      } catch (error) {
        console.error('Error fetching meetings:', error);
        setMeetings([]);
        Analytics.trackBackendConnection(false, error instanceof Error ? error.message : 'Unknown error');
      }
    }
  }, [serverAddress]);

  useEffect(() => {
    fetchMeetings();
  }, [serverAddress, fetchMeetings]);

  useEffect(() => {
    const fetchSettings = async () => {
      setServerAddress('http://localhost:5167');
      setTranscriptServerAddress('http://127.0.0.1:8178/stream');
    };
    fetchSettings();
  }, []);

  const baseItems: SidebarItem[] = [
    {
      id: 'meetings',
      title: 'Meeting Notes',
      type: 'folder' as const,
      children: [
        ...meetings.map(meeting => ({ id: meeting.id, title: meeting.title, created_at: meeting.created_at, started_at: meeting.started_at ?? null, tags: meeting.tags ?? [], type: 'file' as const }))
      ]
    },
  ];


  const toggleCollapse = () => {
    setIsCollapsed(!isCollapsed);
  };

  // Update current meeting when on home page
  useEffect(() => {
    if (pathname === '/') {
      setCurrentMeeting({ id: 'intro-call', title: '+ New Call' });
    }
    setSidebarItems(baseItems);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- baseItems is a new array every render; adding it would reset sidebar items on every render
  }, [pathname]);

  // Update sidebar items when meetings change
  useEffect(() => {
    setSidebarItems(baseItems);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- baseItems is a new array every render; adding it would reset sidebar items on every render
  }, [meetings]);

  // Function to handle recording toggle from sidebar
  const handleRecordingToggle = () => {
    if (!isRecording) {
      // Check if already on home page
      if (pathname === '/') {
        // Already on home - trigger recording directly via custom event
        console.log('Triggering recording from sidebar (already on home page)');
        window.dispatchEvent(new CustomEvent('start-recording-from-sidebar'));
      } else {
        // Not on home - navigate and use auto-start mechanism
        console.log('Navigating to home page with auto-start flag');
        sessionStorage.setItem('autoStartRecording', 'true');
        router.push('/');
      }

      // Track recording initiation from sidebar
      Analytics.trackButtonClick('start_recording', 'sidebar');
    }
    // The actual recording start/stop is handled in the Home component
  };

  // Function to search through meeting transcripts
  const searchTranscripts = async (query: string) => {
    if (!query.trim()) {
      setSearchResults([]);
      return;
    }

    try {
      setIsSearching(true);


      const results = await searchTranscriptsIpc({ query });
      setSearchResults(results);
    } catch (error) {
      console.error('Error searching transcripts:', error);
      setSearchResults([]);
    } finally {
      setIsSearching(false);
    }
  };

  // Summary polling management
  const startSummaryPolling = React.useCallback((
    meetingId: string,
    processId: string,
    onUpdate: (result: any) => void | Promise<void>
  ) => {
    const polls = summaryPollsRef.current;

    // Stop existing poll for this meeting if any
    const existing = polls.get(meetingId);
    if (existing) {
      clearInterval(existing.timer);
    }

    console.log(`📊 Starting polling for meeting ${meetingId}, process ${processId}`);

    let pollCount = 0;
    const MAX_POLLS = 200; // ~16.5 minutes at 5-second intervals (slightly longer than backend's 15-min timeout to avoid race conditions)

    const entry: SummaryPoll = { processId, timer: undefined as unknown as ReturnType<typeof setInterval>, inFlight: false };
    const isCurrent = () => polls.get(meetingId) === entry;
    const finish = () => {
      clearInterval(entry.timer);
      if (isCurrent()) {
        polls.delete(meetingId);
      }
    };
    // Reports one error status and stops; a throwing callback cannot keep the poll alive.
    const failAndStop = async (message: string) => {
      finish();
      try {
        await onUpdate({ status: 'error', error: message });
      } catch (callbackError) {
        console.error(`Summary polling error callback failed for ${meetingId}:`, callbackError);
      }
    };

    entry.timer = setInterval(async () => {
      if (!isCurrent() || entry.inFlight) {
        return;
      }
      pollCount++;

      // Timeout safety: Stop after MAX_POLLS iterations
      if (pollCount >= MAX_POLLS) {
        console.warn(`⏱️ Polling timeout for ${meetingId} after ${MAX_POLLS} iterations`);
        await failAndStop('Summary generation timed out after 15 minutes. Please try again or check your model configuration.');
        return;
      }

      entry.inFlight = true;
      try {
        const result = await getSummary({
          meetingId: meetingId,
        });
        if (!isCurrent()) {
          return;
        }

        console.log(`📊 Polling update for ${meetingId}:`, result.status);

        if (result.status === 'idle') {
          // No run at all: if we get 'idle' after polling started, process completed/disappeared
          if (pollCount > 1) {
            console.log(`Process completed or not found for ${meetingId}, stopping poll`);
            finish();
          }
          return;
        }
        if (result.start !== processId) {
          console.log(`Ignoring summary status for another run of ${meetingId} (${result.start})`);
          return;
        }

        // Call the update callback with result
        await onUpdate(result);

        // Stop polling if completed, error, failed, or cancelled
        if (result.status === 'completed' || result.status === 'error' || result.status === 'failed' || result.status === 'cancelled') {
          console.log(`Polling completed for ${meetingId}, status: ${result.status}`);
          finish();
        }
      } catch (error) {
        console.error(`Polling error for ${meetingId}:`, error);
        if (isCurrent()) {
          // Report error to callback
          await failAndStop(error instanceof Error ? error.message : 'Unknown error');
        }
      } finally {
        entry.inFlight = false;
      }
    }, 5000); // Poll every 5 seconds

    polls.set(meetingId, entry);
  }, []);

  const stopSummaryPolling = React.useCallback((meetingId: string, processId?: string) => {
    const polls = summaryPollsRef.current;
    const entry = polls.get(meetingId);
    if (!entry || (processId !== undefined && entry.processId !== processId)) {
      return;
    }
    console.log(`⏹️ Stopping polling for meeting ${meetingId}`);
    clearInterval(entry.timer);
    polls.delete(meetingId);
  }, []);

  // Cleanup all polling intervals on unmount
  useEffect(() => {
    const polls = summaryPollsRef.current;
    return () => {
      console.log('🧹 Cleaning up all summary polling intervals');
      polls.forEach(entry => clearInterval(entry.timer));
      polls.clear();
    };
  }, []);



  return (
    <SidebarContext.Provider value={{
      currentMeeting,
      setCurrentMeeting,
      sidebarItems,
      isCollapsed,
      toggleCollapse,
      meetings,
      setMeetings,
      isMeetingActive,
      setIsMeetingActive,
      handleRecordingToggle,
      searchTranscripts,
      searchResults,
      isSearching,
      setServerAddress,
      serverAddress,
      transcriptServerAddress,
      setTranscriptServerAddress,
      startSummaryPolling,
      stopSummaryPolling,
      refetchMeetings: fetchMeetings,

    }}>
      {children}
    </SidebarContext.Provider>
  );
}
