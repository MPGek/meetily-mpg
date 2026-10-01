'use client';

import React, { createContext, useContext, useState, useEffect, useRef, useCallback } from 'react';
import {
  builtinAiGetRecommendedModel,
  builtinAiIsModelReady,
  builtinAiDownloadModel,
  parakeetInit,
  parakeetHasAvailableModels,
  parakeetDownloadModel,
  parakeetRetryDownload,
  parakeetGetAvailableModels,
  listenParakeetModelDownloadProgress,
  listenParakeetModelDownloadComplete,
  listenParakeetModelDownloadError,
  listenBuiltinAiDownloadProgress,
} from '@/lib/ipc/models';
import {
  checkFirstLaunch,
  getOnboardingStatus,
  saveOnboardingStatusCmd,
  completeOnboarding as completeOnboardingCmd,
  type OnboardingStatus,
} from '@/lib/ipc/onboarding';
import {
  checkHomebrewDatabase,
  checkDefaultLegacyDatabase,
  importAndInitializeDatabase,
  initializeFreshDatabase,
} from '@/lib/ipc/meetings';
import type { PermissionStatus, OnboardingPermissions } from '@/types/onboarding';
import { resolveOnboardingSummaryModelStatus } from '@/lib/onboarding-summary-model';

const PARAKEET_MODEL = 'parakeet-tdt-0.6b-v3-int8';

interface SummaryModelProgressInfo {
  percent: number;
  downloadedMb: number;
  totalMb: number;
  speedMbps: number;
}

interface ParakeetProgressInfo {
  percent: number;
  downloadedMb: number;
  totalMb: number;
  speedMbps: number;
}

interface OnboardingContextType {
  currentStep: number;
  parakeetDownloaded: boolean;
  parakeetProgress: number;
  parakeetProgressInfo: ParakeetProgressInfo;
  summaryModelDownloaded: boolean;
  summaryModelProgress: number;
  summaryModelProgressInfo: SummaryModelProgressInfo;
  selectedSummaryModel: string;
  recommendedSummaryModel: string;
  databaseExists: boolean;
  isBackgroundDownloading: boolean;
  // Permissions
  permissions: OnboardingPermissions;
  permissionsSkipped: boolean;
  // Navigation
  goToStep: (step: number) => void;
  goNext: () => void;
  goPrevious: () => void;
  // Setters
  setParakeetDownloaded: (value: boolean) => void;
  setSummaryModelDownloaded: (value: boolean) => void;
  setSelectedSummaryModel: (value: string) => void;
  setDatabaseExists: (value: boolean) => void;
  setPermissionStatus: (permission: keyof OnboardingPermissions, status: PermissionStatus) => void;
  setPermissionsSkipped: (skipped: boolean) => void;
  completeOnboarding: () => Promise<void>;
  startBackgroundDownloads: (options: StartBackgroundDownloadsOptions) => Promise<void>;
  retryParakeetDownload: () => Promise<void>;
}

interface StartBackgroundDownloadsOptions {
  includeParakeet: boolean;
  includeSummary: boolean;
  summaryModel?: string;
}

const OnboardingContext = createContext<OnboardingContextType | undefined>(undefined);

export function OnboardingProvider({ children }: { children: React.ReactNode }) {
  const [currentStep, setCurrentStep] = useState(1);
  const [completed, setCompleted] = useState(false);
  const [parakeetDownloaded, setParakeetDownloaded] = useState(false);
  const [parakeetProgress, setParakeetProgress] = useState(0);
  const [parakeetProgressInfo, setParakeetProgressInfo] = useState<ParakeetProgressInfo>({
    percent: 0,
    downloadedMb: 0,
    totalMb: 0,
    speedMbps: 0,
  });
  const [summaryModelDownloaded, setSummaryModelDownloaded] = useState(false);
  const [summaryModelProgress, setSummaryModelProgress] = useState(0);
  const [summaryModelProgressInfo, setSummaryModelProgressInfo] = useState<SummaryModelProgressInfo>({
    percent: 0,
    downloadedMb: 0,
    totalMb: 0,
    speedMbps: 0,
  });
  const [selectedSummaryModel, setSelectedSummaryModel] = useState<string>('');
  const [recommendedSummaryModel, setRecommendedSummaryModel] = useState<string>('');
  const [databaseExists, setDatabaseExists] = useState(false);
  const [isBackgroundDownloading, setIsBackgroundDownloading] = useState(false);

  // Permissions state
  const [permissions, setPermissions] = useState<OnboardingPermissions>({
    microphone: 'not_determined',
    systemAudio: 'not_determined',
    screenRecording: 'not_determined',
  });
  const [permissionsSkipped, setPermissionsSkipped] = useState(false);

  const saveTimeoutRef = useRef<NodeJS.Timeout>();

  const initializeSummaryModelSelection = async (preferredModel = selectedSummaryModel) => {
    try {
      const recommendedModel = await builtinAiGetRecommendedModel();
      setRecommendedSummaryModel(recommendedModel);
      const modelToCheck = preferredModel || recommendedModel;
      setSelectedSummaryModel(modelToCheck);

      const selectedModelReady = await builtinAiIsModelReady({
        modelName: modelToCheck,
        refresh: true,
      });
      const resolved = resolveOnboardingSummaryModelStatus({
        selectedModel: preferredModel,
        recommendedModel,
        selectedModelReady,
      });

      setSelectedSummaryModel(resolved.selectedSummaryModel);
      setSummaryModelDownloaded(resolved.summaryModelDownloaded);
      console.log('[OnboardingContext] Set recommended model:', resolved.selectedSummaryModel);

      return resolved;
    } catch (error) {
      console.error('[OnboardingContext] Failed to initialize summary model:', error);
      return null;
    }
  };

  const requestSummaryModelDownload = (modelName: string) => {
    console.log('[OnboardingContext] Starting Summary Model download');
    builtinAiDownloadModel({ modelName })
      .catch(err => {
        if (String(err).includes('Download already in progress')) {
          return;
        }
        console.error('[OnboardingContext] Summary Model download failed:', err);
      });
  };

  // Load status on mount and initialize database
  useEffect(() => {
    loadOnboardingStatus();
    checkDatabaseStatus();
    initializeDatabaseInBackground();
  // eslint-disable-next-line react-hooks/exhaustive-deps -- mount-only load and DB init; both functions are recreated every render and would re-run initialization
  }, []);

  // Initialize database silently in background (moved from SetupOverviewStep)
  const initializeDatabaseInBackground = async () => {
    try {
      console.log('[OnboardingContext] Starting background database initialization');
      const isFirstLaunch = await checkFirstLaunch();

      if (!isFirstLaunch) {
        console.log('[OnboardingContext] Database exists, skipping initialization');
        setDatabaseExists(true);
        return;
      }

      // First launch - attempt auto-detection and import
      await performAutoDetection();
    } catch (error) {
      console.error('[OnboardingContext] Database initialization failed:', error);
      // Don't throw - database init failure shouldn't block onboarding
    }
  };

  const performAutoDetection = async () => {
    // Check Homebrew (macOS only)
    if (typeof navigator !== 'undefined' && navigator.platform?.toLowerCase().includes('mac')) {
      const homebrewDbPath = '/usr/local/var/meetily/meeting_minutes.db';
      try {
        const homebrewCheck = await checkHomebrewDatabase(
          { path: homebrewDbPath }
        );

        if (homebrewCheck?.exists) {
          console.log('[OnboardingContext] Found Homebrew database, importing');
          await importAndInitializeDatabase({ legacyDbPath: homebrewDbPath });
          setDatabaseExists(true);
          return;
        }
      } catch (e) {
        console.log('[OnboardingContext] Homebrew check failed, continuing:', e);
      }
    }

    // Check default legacy database location
    try {
      const legacyPath = await checkDefaultLegacyDatabase();
      if (legacyPath) {
        console.log('[OnboardingContext] Found legacy database, importing');
        await importAndInitializeDatabase({ legacyDbPath: legacyPath });
        setDatabaseExists(true);
        return;
      }
    } catch (e) {
      console.log('[OnboardingContext] Legacy check failed, continuing:', e);
    }

    // No legacy database found - initialize fresh
    console.log('[OnboardingContext] No legacy database found, initializing fresh');
    await initializeFreshDatabase();
    setDatabaseExists(true);
  };

  const isCompletingRef = useRef(false);

  // Auto-save on state change (debounced)
  useEffect(() => {
    if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);

    // Don't auto-save if completed (to avoid overwriting completion status)
    // Also don't auto-save if we are currently in the process of completing
    if (completed || isCompletingRef.current) return;

    saveTimeoutRef.current = setTimeout(() => {
      saveOnboardingStatus();
    }, 1000);

    return () => {
      if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- debounced auto-save keyed on the saved fields; saveOnboardingStatus is recreated every render and would reschedule the save on every render
  }, [currentStep, parakeetDownloaded, summaryModelDownloaded, completed]);

  // Listen to Parakeet download progress
  useEffect(() => {
    const unlisten = listenParakeetModelDownloadProgress(
      (event) => {
        const { modelName, progress, downloaded_mb, total_mb, speed_mbps, status } = event.payload;
        if (modelName === PARAKEET_MODEL) {
          setParakeetProgress(progress);
          setParakeetProgressInfo({
            percent: progress,
            downloadedMb: downloaded_mb ?? 0,
            totalMb: total_mb ?? 0,
            speedMbps: speed_mbps ?? 0,
          });
          if (status === 'completed' || progress >= 100) {
            setParakeetDownloaded(true);
          }
        }
      }
    );

    const unlistenComplete = listenParakeetModelDownloadComplete(
      (event) => {
        const { modelName } = event.payload;
        if (modelName === PARAKEET_MODEL) {
          setParakeetDownloaded(true);
          setParakeetProgress(100);
        }
      }
    );

    const unlistenError = listenParakeetModelDownloadError(
      (event) => {
        const { modelName } = event.payload;
        if (modelName === PARAKEET_MODEL) {
          console.error('Parakeet download error:', event.payload.error);
        }
      }
    );

    return () => {
      unlisten.then(fn => fn());
      unlistenComplete.then(fn => fn());
      unlistenError.then(fn => fn());
    };
  }, []);

  // Listen to summary model (Built-in AI) download progress
  useEffect(() => {
    const unlisten = listenBuiltinAiDownloadProgress(
      (event) => {
        const { model, progress, downloaded_mb, total_mb, speed_mbps, status } = event.payload;
        if (selectedSummaryModel && model === selectedSummaryModel) {
          setSummaryModelProgress(progress);
          setSummaryModelProgressInfo({
            percent: progress,
            downloadedMb: downloaded_mb ?? 0,
            totalMb: total_mb ?? 0,
            speedMbps: speed_mbps ?? 0,
          });
          if (status === 'completed' || progress >= 100) {
            setSummaryModelDownloaded(true);
          }
        }
      }
    );

    return () => {
      unlisten.then(fn => fn());
    };
  }, [selectedSummaryModel]);

  const checkDatabaseStatus = async () => {
    try {
      const isFirstLaunch = await checkFirstLaunch();
      setDatabaseExists(!isFirstLaunch);
      console.log('[OnboardingContext] Database exists:', !isFirstLaunch);
    } catch (error) {
      console.error('[OnboardingContext] Failed to check database status:', error);
      setDatabaseExists(false);
    }
  };

  const loadOnboardingStatus = async () => {
    try {
      const status = await getOnboardingStatus();
      if (status) {
        console.log('[OnboardingContext] Loaded saved status:', status);

        if (status.completed) {
          setCurrentStep(status.current_step);
          setCompleted(true);
          setParakeetDownloaded(status.model_status.parakeet === 'downloaded');
          setSummaryModelDownloaded(status.model_status.summary === 'downloaded');
          if (status.model_status.selected_summary_model) {
            setSelectedSummaryModel(status.model_status.selected_summary_model);
          }
          console.log('[OnboardingContext] Restored completed onboarding status without model verification');
          return;
        }

        // Don't trust saved status - verify actual model status on disk
        const verifiedStatus = await verifyModelStatus(status);

        setCurrentStep(verifiedStatus.currentStep);
        setCompleted(verifiedStatus.completed);
        setParakeetDownloaded(verifiedStatus.parakeetDownloaded);
        setSummaryModelDownloaded(verifiedStatus.summaryModelDownloaded);
        if (verifiedStatus.selectedSummaryModel) {
          setSelectedSummaryModel(verifiedStatus.selectedSummaryModel);
        }

        console.log('[OnboardingContext] Verified status:', verifiedStatus);

        // Check if any downloads are active to restore isBackgroundDownloading state
        await checkActiveDownloads();
      } else {
        await initializeSummaryModelSelection();
      }
    } catch (error) {
      console.error('[OnboardingContext] Failed to load onboarding status:', error);
    }
  };

  // Verify that models actually exist on disk, not just trust saved JSON
  const verifyModelStatus = async (savedStatus: OnboardingStatus) => {
    let parakeetDownloaded = false;
    let summaryModelDownloaded = false;
    let selectedSummaryModel = '';

    // Verify Parakeet model exists on disk
    try {
      await parakeetInit();
      parakeetDownloaded = await parakeetHasAvailableModels();
      console.log('[OnboardingContext] Parakeet verified on disk:', parakeetDownloaded);
    } catch (error) {
      console.warn('[OnboardingContext] Failed to verify Parakeet:', error);
      parakeetDownloaded = false;
    }

    // Verify the selected/recommended Summary model exists on disk.
    try {
      const recommendedModel = await builtinAiGetRecommendedModel();
      setRecommendedSummaryModel(recommendedModel);
      const savedSelectedModel = savedStatus.model_status.selected_summary_model || '';
      const modelToCheck = savedSelectedModel || recommendedModel;
      const selectedModelReady = await builtinAiIsModelReady({
        modelName: modelToCheck,
        refresh: true,
      });
      const resolved = resolveOnboardingSummaryModelStatus({
        selectedModel: savedSelectedModel,
        recommendedModel,
        selectedModelReady,
      });
      selectedSummaryModel = resolved.selectedSummaryModel;
      summaryModelDownloaded = resolved.summaryModelDownloaded;
      console.log('[OnboardingContext] Summary model verified on disk:', summaryModelDownloaded, 'model:', selectedSummaryModel);
    } catch (error) {
      console.warn('[OnboardingContext] Failed to verify Summary model:', error);
      summaryModelDownloaded = false;
    }

    // Determine the correct step based on verified status
    // New simplified flow: Step 1: Welcome, Step 2: Setup Overview, Step 3: Download Progress, Step 4: Permissions (macOS)
    let currentStep = savedStatus.current_step;
    const completed = savedStatus.completed;

    // Clamp step to new max (4)
    if (currentStep > 4) {
      currentStep = 3; // Go to download progress step
    }

    // Trust the completed status - don't revert based on model downloads
    // Downloads continue in background; user stays in main app regardless
    return {
      currentStep,
      completed,
      parakeetDownloaded,
      summaryModelDownloaded,
      selectedSummaryModel,
    };
  };

  const saveOnboardingStatus = async () => {
    // Safety check: if we are in the process of completing, DO NOT save
    // This prevents a race condition where a download completion event triggers a save
    // that overwrites the "completed" status set by completeOnboarding
    if (isCompletingRef.current) {
      console.log('[OnboardingContext] Skipping saveOnboardingStatus because completion is in progress');
      return;
    }

    try {
      await saveOnboardingStatusCmd({
        status: {
          version: '1.0',
          completed: completed,
          current_step: currentStep,
          model_status: {
            parakeet: parakeetDownloaded ? 'downloaded' : 'not_downloaded',
            summary: summaryModelDownloaded ? 'downloaded' : 'not_downloaded',
            selected_summary_model: selectedSummaryModel || undefined,
          },
          last_updated: new Date().toISOString(),
        },
      });
    } catch (error) {
      console.error('[OnboardingContext] Failed to save onboarding status:', error);
    }
  };

  const completeOnboarding = async () => {
    try {
      // Set completion flag to prevent race conditions with auto-save
      isCompletingRef.current = true;

      // Clear any pending auto-saves
      if (saveTimeoutRef.current) {
        clearTimeout(saveTimeoutRef.current);
        saveTimeoutRef.current = undefined;
      }

      let modelToSave = selectedSummaryModel;
      if (!modelToSave) {
        modelToSave = await builtinAiGetRecommendedModel();
        setSelectedSummaryModel(modelToSave);
      }

      const selectedModelReady = await builtinAiIsModelReady({
        modelName: modelToSave,
        refresh: true,
      });
      setSummaryModelDownloaded(selectedModelReady);
      if (!selectedModelReady) {
        requestSummaryModelDownload(modelToSave);
      }

      // Onboarding always uses builtin-ai with selected model
      await completeOnboardingCmd({
        model: modelToSave,
      });
      setCompleted(true);
      console.log('[OnboardingContext] Onboarding completed with model:', modelToSave);

      // Reset the flag so subsequent state updates can be saved
      isCompletingRef.current = false;
    } catch (error) {
      console.error('[OnboardingContext] Failed to complete onboarding:', error);
      isCompletingRef.current = false; // Reset flag on error
      throw error; // Re-throw so PermissionsStep can handle it
    }
  };

  // Start background downloads for models.
  const startBackgroundDownloads = async ({
    includeParakeet,
    includeSummary,
    summaryModel,
  }: StartBackgroundDownloadsOptions) => {
    console.log('[OnboardingContext] Starting background downloads:', {
      includeParakeet,
      includeSummary,
      summaryModel,
    });

    try {
      const shouldStartParakeet = includeParakeet && !parakeetDownloaded;
      const shouldStartSummary = includeSummary && !summaryModelDownloaded && !!summaryModel;

      if (!shouldStartParakeet && !shouldStartSummary) {
        if (includeSummary && !summaryModelDownloaded && !summaryModel) {
          console.warn('[OnboardingContext] Summary Model download skipped until recommendation is loaded');
        }
        return;
      }

      setIsBackgroundDownloading(true);

      // Start Parakeet download first (speech recognition - always required)
      if (shouldStartParakeet) {
        console.log('[OnboardingContext] Starting Parakeet download');
        parakeetDownloadModel({ modelName: PARAKEET_MODEL })
          .catch(err => console.error('[OnboardingContext] Parakeet download failed:', err));
      }

      // Start selected Summary Model download immediately so completion cannot race the request.
      if (shouldStartSummary && summaryModel) {
        requestSummaryModelDownload(summaryModel);
      }
    } catch (error) {
      console.error('[OnboardingContext] Failed to start background downloads:', error);
      setIsBackgroundDownloading(false);
      throw error;
    }
  };

  // Check if any models are currently downloading (for re-entry)
  const checkActiveDownloads = async () => {
    try {
      const models: any[] = await parakeetGetAvailableModels();
      const isDownloading = models.some(m => m.status && (typeof m.status === 'object' ? 'Downloading' in m.status : m.status === 'Downloading'));
      
      if (isDownloading) {
        console.log('[OnboardingContext] Detected active background downloads on mount');
        setIsBackgroundDownloading(true);
      }
      
      // Also check for Built-in AI downloads if possible (though less critical as Parakeet is the main blocker)
      
    } catch (error) {
      console.warn('[OnboardingContext] Failed to check active downloads:', error);
    }
  };

  const retryParakeetDownload = async () => {
    console.log('[OnboardingContext] Retrying Parakeet download');
    try {
      await parakeetRetryDownload({ modelName: PARAKEET_MODEL });
    } catch (error) {
      console.error('[OnboardingContext] Retry failed:', error);
      throw error;
    }
  };

  const setPermissionStatus = useCallback((permission: keyof OnboardingPermissions, status: PermissionStatus) => {
    setPermissions((prev: OnboardingPermissions) => ({
      ...prev,
      [permission]: status,
    }));
  }, []);

  const goToStep = useCallback((step: number) => {
    setCurrentStep(Math.max(1, Math.min(step, 4)));
  }, []);

  const goNext = useCallback(() => {
    setCurrentStep((prev: number) => {
      const next = prev + 1;
      // Don't go past step 4
      return Math.min(next, 4);
    });
  }, []);

  const goPrevious = useCallback(() => {
    setCurrentStep((prev: number) => {
      const previous = prev - 1;
      // Don't go below step 1
      return Math.max(previous, 1);
    });
  }, []);

  return (
    <OnboardingContext.Provider
      value={{
        currentStep,
        parakeetDownloaded,
        parakeetProgress,
        parakeetProgressInfo,
        summaryModelDownloaded,
        summaryModelProgress,
        summaryModelProgressInfo,
        selectedSummaryModel,
        recommendedSummaryModel,
        databaseExists,
        isBackgroundDownloading,
        permissions,
        permissionsSkipped,
        goToStep,
        goNext,
        goPrevious,
        setParakeetDownloaded,
        setSummaryModelDownloaded,
        setSelectedSummaryModel,
        setDatabaseExists,
        setPermissionStatus,
        setPermissionsSkipped,
        completeOnboarding,
        startBackgroundDownloads,
        retryParakeetDownload,
      }}
    >
      {children}
    </OnboardingContext.Provider>
  );
}

export function useOnboarding() {
  const context = useContext(OnboardingContext);
  if (!context) {
    throw new Error('useOnboarding must be used within OnboardingProvider');
  }
  return context;
}
