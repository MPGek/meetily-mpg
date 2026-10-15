// Adapted from upstream meetily e4cc94b frontend/tests/hooks/summary-generation.test.tsx
// to the fork's typed IPC (lib/ipc/summary.ts) and hook surface (initialSummary).
import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { useEffect, useState } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { SummaryStatusResponse } from '../../src/lib/ipc/summary';
import type { Summary } from '../../src/types';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Bun shares module mocks between test files; restore the originals after this suite.
const originalCore = { ...await import('@tauri-apps/api/core') };
const originalEvent = { ...await import('@tauri-apps/api/event') };
const originalToast = { ...await import('sonner') };
const originalAnalytics = { ...await import('../../src/lib/analytics') };
const originalPreferences = { ...await import('../../src/lib/summary-language-preferences') };
const originalRecordingState = { ...await import('../../src/contexts/RecordingStateContext') };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
  mock.module('@tauri-apps/api/event', () => originalEvent);
  mock.module('sonner', () => originalToast);
  mock.module('../../src/lib/analytics', () => originalAnalytics);
  mock.module('../../src/lib/summary-language-preferences', () => originalPreferences);
  mock.module('../../src/contexts/RecordingStateContext', () => originalRecordingState);
});

mock.module('next/navigation', () => ({ usePathname: () => '/meeting-details', useRouter: () => ({}) }));
mock.module('../../src/contexts/RecordingStateContext', () => ({ useRecordingState: () => ({ isRecording: false }) }));
const notify = mock(() => {});
mock.module('sonner', () => ({ toast: { info: notify, error: notify, success: notify, warning: notify } }));
const trackCompletion = mock(async () => {});
mock.module('../../src/lib/analytics', () => ({
  default: {
    trackBackendConnection() {},
    trackSummaryGenerationStarted: async () => {},
    trackCustomPromptUsed: async () => {},
    trackSummaryGenerationCompleted: trackCompletion,
  },
}));
mock.module('../../src/lib/summary-language-preferences', () => ({
  readCachedDetectedSummaryLanguage: async () => null,
  detectAndCacheSummaryLanguage: async () => ({ language: 'en', reason: 'detected' }),
  readMeetingSummaryLanguage: async () => ({ language: 'en', storage: 'metadata' }),
}));

const RUN_A = '2026-10-02T10:00:00.000000001Z';
const RUN_OLD = '2026-10-02T09:00:00.000000001Z';

let startProcess: () => Promise<{ message: string; process_id: string }>;
let getSummary: (meetingId: string) => Promise<SummaryStatusResponse>;
const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'api_get_meetings') return [];
  if (command === 'api_get_summary') return getSummary(args!.meetingId as string);
  if (command === 'api_get_meeting_transcripts') {
    return { transcripts: [{ id: 't1', text: 'Meeting transcript', timestamp: '00:00' }], total_count: 1 };
  }
  if (command === 'get_ollama_models') return [{ name: 'test' }];
  if (command === 'api_process_transcript') return startProcess();
  if (command === 'api_cancel_summary') return { message: 'Summary generation cancelled successfully', meeting_id: args!.meetingId };
  throw new Error(`Unexpected command: ${command}`);
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('@tauri-apps/api/event', () => ({ listen: mock(async () => () => {}), emit: mock(async () => {}) }));

const { SidebarProvider, useSidebar } = await import('../../src/components/Sidebar/SidebarProvider');
const { shouldAutoStartSummary, useSummaryGeneration } = await import('../../src/hooks/meeting-details/useSummaryGeneration');

const response = (overrides: Partial<SummaryStatusResponse> = {}): SummaryStatusResponse => ({
  meeting_id: 'meeting-a', status: 'pending', start: RUN_A, end: null,
  data: null, error: null, meetingName: 'Meeting A', ...overrides,
});

let state: ReturnType<typeof useSummaryGeneration>;
function Status({ initialSummary, meetingId, autoGenerate }: {
  initialSummary: SummaryStatusResponse; meetingId: string; autoGenerate: boolean;
}) {
  const [summary, setAiSummary] = useState<Summary | null>(() => initialSummary.data as Summary | null);
  const [title, updateMeetingTitle] = useState('Original title');
  state = useSummaryGeneration({
    meeting: { id: meetingId, title, created_at: '2026-09-01T00:00:00Z', updated_at: '2026-09-01T00:00:00Z' },
    transcripts: [],
    modelConfig: { provider: 'ollama', model: 'test', whisperModel: 'base' } as never,
    isModelConfigLoading: false,
    selectedTemplate: 'daily_standup',
    setAiSummary, updateMeetingTitle, initialSummary,
  });
  // Same gate as page-content.tsx's auto-generate effect.
  const { summaryStatus, handleGenerateSummary } = state;
  useEffect(() => {
    if (shouldAutoStartSummary(autoGenerate, 1, summaryStatus)) {
      void handleGenerateSummary('');
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps -- mirrors page-content: fires once per mount
  }, []);
  return <output>{state.summaryStatus}:{state.summaryError}:{title}:{JSON.stringify(summary)}</output>;
}

let renderer: ReactTestRenderer | undefined;
const timers = new Map<number, () => Promise<void>>();
let nextTimer = 0;
const realSetInterval = globalThis.setInterval;
const realClearInterval = globalThis.clearInterval;
beforeEach(() => {
  renderer = undefined;
  timers.clear(); notify.mockClear(); trackCompletion.mockClear(); invoke.mockClear();
  getSummary = async () => response();
  startProcess = async () => ({ message: 'Summary generation started', process_id: RUN_A });
  globalThis.setInterval = ((callback: () => Promise<void>) => {
    const id = ++nextTimer; timers.set(id, callback); return id;
  }) as unknown as typeof setInterval;
  globalThis.clearInterval = ((id: number) => { timers.delete(id); }) as unknown as typeof clearInterval;
});
afterEach(async () => {
  if (renderer) {
    const mounted = renderer;
    await act(async () => mounted.unmount());
  }
  globalThis.setInterval = realSetInterval;
  globalThis.clearInterval = realClearInterval;
});
async function show(initialSummary: SummaryStatusResponse | null, meetingId = 'meeting-a', autoGenerate = false) {
  await act(async () => {
    const view = (
      <SidebarProvider>
        {initialSummary && <Status key={meetingId} initialSummary={initialSummary} meetingId={meetingId} autoGenerate={autoGenerate} />}
      </SidebarProvider>
    );
    if (renderer) renderer.update(view); else renderer = create(view);
  });
}
const text = () => JSON.stringify(renderer!.toJSON());
async function tick() {
  await act(async () => { for (const callback of [...timers.values()]) await callback(); });
}
const calls = (command: string) => invoke.mock.calls.filter(([name]) => name === command);

describe('summary state restored when returning to a meeting', () => {
  test('resumes pending generation after leaving and returning, then shows completion and the new title', async () => {
    await show(response());
    expect(state.summaryStatus).toBe('processing');
    await show(null);
    expect(timers.size).toBe(0);
    await show(response());
    expect(state.summaryStatus).toBe('processing');
    getSummary = async () => response({ status: 'completed', data: { markdown: 'Finished summary' }, meetingName: 'New title' });
    await tick();
    expect(state.summaryStatus).toBe('completed');
    expect(text()).toContain('Finished summary');
    expect(text()).toContain('New title');
    expect(timers.size).toBe(0);
    // A resumed run's outcome is not attributed to this visit.
    expect(trackCompletion).not.toHaveBeenCalled();
  });

  test('regeneration resume keeps the old notes and restores them on failure', async () => {
    await show(response({ data: { markdown: 'Previous summary' } }));
    expect(state.summaryStatus).toBe('regenerating');
    expect(text()).toContain('Previous summary');
    getSummary = async () => response({ status: 'failed', error: 'Model unavailable', data: { markdown: 'Previous summary' } });
    await tick();
    expect(state.summaryStatus).toBe('completed');
    expect(text()).toContain('Previous summary');
    expect(timers.size).toBe(0);
  });

  test('Stop after returning cancels the resumed run by its process id', async () => {
    await show(response());
    await act(async () => state.handleStopGeneration());
    expect(calls('api_cancel_summary')).toEqual([['api_cancel_summary', { meetingId: 'meeting-a', processId: RUN_A }]]);
    expect(state.summaryStatus).toBe('idle');
    expect(timers.size).toBe(0);
  });

  test('an ordinary re-render keeps the existing poll', async () => {
    const initial = response();
    await show(initial);
    const timer = [...timers.keys()];
    expect(timer).toHaveLength(1);
    await show(initial);
    expect([...timers.keys()]).toEqual(timer);
    getSummary = async () => response({ status: 'completed', data: { markdown: 'Finished summary' } });
    await tick();
    expect(state.summaryStatus).toBe('completed');
  });

  test('a failure that happened while away shows the stored error without a toast', async () => {
    await show(response({ status: 'failed', error: 'Model unavailable', data: { markdown: 'Previous summary' } }));
    expect(state.summaryStatus).toBe('error');
    expect(state.summaryError).toBe('Model unavailable');
    expect(text()).toContain('Previous summary');
    expect(timers.size).toBe(0);
    expect(notify).not.toHaveBeenCalled();
  });

  test('a run completed or cancelled while away shows the stored summary without toast or analytics', async () => {
    await show(response({ status: 'completed', data: { markdown: 'Finished while away' } }));
    expect(state.summaryStatus).toBe('idle');
    expect(text()).toContain('Finished while away');
    await show(null);
    await show(response({ status: 'cancelled', data: { markdown: 'Restored summary' } }));
    expect(state.summaryStatus).toBe('idle');
    expect(text()).toContain('Restored summary');
    expect(timers.size).toBe(0);
    expect(notify).not.toHaveBeenCalled();
    expect(trackCompletion).not.toHaveBeenCalled();
  });

  test('a pending stored status blocks auto-generation and tracks the pending run', async () => {
    await show(response(), 'meeting-a', true);
    expect(calls('api_process_transcript')).toEqual([]);
    expect(state.summaryStatus).toBe('processing');
    expect(timers.size).toBe(1);
  });

  test('an idle stored status auto-generates exactly once', async () => {
    await show(response({ status: 'idle', start: null }), 'meeting-a', true);
    await act(async () => { for (let i = 0; i < 20; i++) await Promise.resolve(); });
    expect(calls('api_process_transcript')).toHaveLength(1);
    expect(timers.size).toBe(1);
  });

  test('a stored status for another meeting stays idle and starts no poll', async () => {
    await show(response({ meeting_id: 'meeting-b' }));
    expect(state.summaryStatus).toBe('idle');
    expect(timers.size).toBe(0);
  });

  test('a late completion from meeting A does not change meeting B', async () => {
    let resolve!: (value: SummaryStatusResponse) => void;
    getSummary = () => new Promise(done => { resolve = done; });
    await show(response());
    await act(async () => { for (const callback of [...timers.values()]) void callback(); });
    await show(response({ meeting_id: 'meeting-b', status: 'idle', start: null }), 'meeting-b');
    await act(async () => resolve(response({ status: 'completed', data: { markdown: 'Wrong meeting' } })));
    expect(state.summaryStatus).toBe('idle');
    expect(text()).not.toContain('Wrong meeting');
    expect(notify).not.toHaveBeenCalled();
  });

  test('an old in-flight poll does not stop the resumed poll for the same run', async () => {
    let resolve!: (value: SummaryStatusResponse) => void;
    getSummary = () => new Promise(done => { resolve = done; });
    await show(response());
    await act(async () => { for (const callback of [...timers.values()]) void callback(); });
    await show(null);
    await show(response());
    await act(async () => resolve(response({ status: 'completed', data: { markdown: 'Finished summary' } })));
    expect(state.summaryStatus).toBe('processing');
    expect(timers.size).toBe(1);
    getSummary = async () => response({ status: 'completed', data: { markdown: 'Finished summary' } });
    await tick();
    expect(state.summaryStatus).toBe('completed');
    expect(text()).toContain('Finished summary');
  });

  test('leaving before the start response arrives sends no cancel, and a later visit resumes', async () => {
    let resolve: ((value: { message: string; process_id: string }) => void) | undefined;
    startProcess = () => new Promise(done => { resolve = done; });
    await show(response({ status: 'idle', start: null }));
    let generation!: Promise<void>;
    await act(async () => {
      generation = state.handleGenerateSummary();
      for (let i = 0; i < 50 && !resolve; i++) await Promise.resolve();
    });
    expect(resolve).toBeDefined();
    await show(null);
    await act(async () => { resolve!({ message: 'Summary generation started', process_id: RUN_A }); await generation; });
    expect(calls('api_cancel_summary')).toEqual([]);
    expect(timers.size).toBe(0);
    await show(response());
    expect(state.summaryStatus).toBe('processing');
    getSummary = async () => response({ status: 'completed', data: { markdown: 'Finished summary' } });
    await tick();
    expect(state.summaryStatus).toBe('completed');
  });

  test('a start response that arrives after Stop cancels its own run', async () => {
    let resolve: ((value: { message: string; process_id: string }) => void) | undefined;
    startProcess = () => new Promise(done => { resolve = done; });
    await show(response({ status: 'idle', start: null }));
    let generation!: Promise<void>;
    await act(async () => {
      generation = state.handleGenerateSummary();
      for (let i = 0; i < 50 && !resolve; i++) await Promise.resolve();
    });
    await act(async () => state.handleStopGeneration());
    expect(calls('api_cancel_summary')).toEqual([]);
    await act(async () => { resolve!({ message: 'Summary generation started', process_id: RUN_A }); await generation; });
    expect(calls('api_cancel_summary')).toEqual([['api_cancel_summary', { meetingId: 'meeting-a', processId: RUN_A }]]);
    expect(state.summaryStatus).toBe('idle');
    expect(timers.size).toBe(0);
  });
});

describe('sidebar summary polling', () => {
  type Poll = { meetingId: string; processId: string; onUpdate: (result: unknown) => void | Promise<void> };
  let sidebar: ReturnType<typeof useSidebar>;
  function Consumer({ polls }: { polls: Poll[] }) {
    sidebar = useSidebar();
    const { startSummaryPolling } = sidebar;
    useEffect(() => {
      for (const poll of polls) startSummaryPolling(poll.meetingId, poll.processId, poll.onUpdate);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- start once per mount
    }, [startSummaryPolling]);
    return null;
  }
  async function poll(polls: Poll[]) {
    await act(async () => { renderer = create(<SidebarProvider><Consumer polls={polls} /></SidebarProvider>); });
  }

  test.each(['read', 'callback'])('a failing %s stops the poll even when the error callback also throws', async (failure) => {
    const onUpdate = mock(async (_result: unknown) => { throw new Error('Consumer failed'); });
    await poll([{ meetingId: 'meeting-a', processId: RUN_A, onUpdate }]);
    getSummary = async () => {
      if (failure === 'read') throw new Error('Database unavailable');
      return response({ status: 'pending' });
    };
    await tick();
    expect(timers.size).toBe(0);
    expect(onUpdate.mock.calls.at(-1)?.[0]).toMatchObject({ status: 'error' });
    await tick();
    expect(onUpdate).toHaveBeenCalledTimes(failure === 'read' ? 1 : 2);
  });

  test("one meeting's poll ending does not stop another meeting's poll", async () => {
    const updatesB: unknown[] = [];
    await poll([
      { meetingId: 'meeting-a', processId: RUN_A, onUpdate: () => {} },
      { meetingId: 'meeting-b', processId: RUN_A, onUpdate: (result) => { updatesB.push(result); } },
    ]);
    expect(timers.size).toBe(2);
    getSummary = async (meetingId) => meetingId === 'meeting-a'
      ? response({ status: 'completed', data: { markdown: 'A done' } })
      : response({ meeting_id: 'meeting-b' });
    await tick();
    expect(timers.size).toBe(1);
    await tick();
    expect(updatesB).toHaveLength(2);
  });

  test('a result for another run of the meeting is ignored', async () => {
    const onUpdate = mock(() => {});
    await poll([{ meetingId: 'meeting-a', processId: RUN_A, onUpdate }]);
    getSummary = async () => response({ status: 'completed', start: RUN_OLD, data: { markdown: 'Old run' } });
    await tick();
    expect(onUpdate).not.toHaveBeenCalled();
    expect(timers.size).toBe(1);
  });

  test('a stop naming another run leaves the poll running', async () => {
    await poll([{ meetingId: 'meeting-a', processId: RUN_A, onUpdate: () => {} }]);
    await act(async () => sidebar.stopSummaryPolling('meeting-a', RUN_OLD));
    expect(timers.size).toBe(1);
    await act(async () => sidebar.stopSummaryPolling('meeting-a', RUN_A));
    expect(timers.size).toBe(0);
  });
});
