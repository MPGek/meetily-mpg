/**
 * Typed wrappers for the PostHog analytics commands
 * (src-tauri/src/analytics/commands.rs). Every `track_*` command rejects
 * with "Analytics client not initialized" before `init_analytics` runs.
 */
import { invokeTyped } from './core';

/** Event properties; Rust takes `Option<HashMap<String, String>>`. */
export type AnalyticsEventProperties = Record<string, string>;

// --- Lifecycle ---------------------------------------------------------------

export async function initAnalytics(): Promise<void> {
  return invokeTyped<void>('init_analytics');
}

export async function disableAnalytics(): Promise<void> {
  return invokeTyped<void>('disable_analytics');
}

/** Never rejects on the Rust side: returns false when no client exists. */
export async function isAnalyticsEnabled(): Promise<boolean> {
  return invokeTyped<boolean>('is_analytics_enabled');
}

export interface StartAnalyticsSessionArgs {
  userId: string;
}

/** Resolves to the new session id. */
export async function startAnalyticsSession(args: StartAnalyticsSessionArgs): Promise<string> {
  return invokeTyped<string>('start_analytics_session', args);
}

export async function endAnalyticsSession(): Promise<void> {
  return invokeTyped<void>('end_analytics_session');
}

/** Never rejects on the Rust side: returns false when no client exists. */
export async function isAnalyticsSessionActive(): Promise<boolean> {
  return invokeTyped<boolean>('is_analytics_session_active');
}

// --- Identity and generic events --------------------------------------------

export interface IdentifyUserArgs {
  userId: string;
  properties?: AnalyticsEventProperties | null;
}

export async function identifyUser(args: IdentifyUserArgs): Promise<void> {
  return invokeTyped<void>('identify_user', args);
}

export interface TrackEventArgs {
  eventName: string;
  properties?: AnalyticsEventProperties | null;
}

export async function trackEvent(args: TrackEventArgs): Promise<void> {
  return invokeTyped<void>('track_event', args);
}

export async function trackDailyActiveUser(): Promise<void> {
  return invokeTyped<void>('track_daily_active_user');
}

export async function trackUserFirstLaunch(): Promise<void> {
  return invokeTyped<void>('track_user_first_launch');
}

// --- Consent -----------------------------------------------------------------

export async function trackAnalyticsEnabled(): Promise<void> {
  return invokeTyped<void>('track_analytics_enabled');
}

export async function trackAnalyticsDisabled(): Promise<void> {
  return invokeTyped<void>('track_analytics_disabled');
}

export async function trackAnalyticsTransparencyViewed(): Promise<void> {
  return invokeTyped<void>('track_analytics_transparency_viewed');
}

// --- Meetings and recording --------------------------------------------------

export interface MeetingIdArgs {
  meetingId: string;
}

export async function trackMeetingStarted(args: MeetingIdArgs): Promise<void> {
  return invokeTyped<void>('track_meeting_started', args);
}

export async function trackRecordingStarted(args: MeetingIdArgs): Promise<void> {
  return invokeTyped<void>('track_recording_started', args);
}

export interface TrackRecordingStoppedArgs {
  meetingId: string;
  durationSeconds?: number | null;
}

export async function trackRecordingStopped(args: TrackRecordingStoppedArgs): Promise<void> {
  return invokeTyped<void>('track_recording_stopped', args);
}

export async function trackMeetingDeleted(args: MeetingIdArgs): Promise<void> {
  return invokeTyped<void>('track_meeting_deleted', args);
}

// --- Settings and features ---------------------------------------------------

export interface TrackSettingsChangedArgs {
  settingType: string;
  newValue: string;
}

export async function trackSettingsChanged(args: TrackSettingsChangedArgs): Promise<void> {
  return invokeTyped<void>('track_settings_changed', args);
}

export interface TrackFeatureUsedArgs {
  featureName: string;
}

export async function trackFeatureUsed(args: TrackFeatureUsedArgs): Promise<void> {
  return invokeTyped<void>('track_feature_used', args);
}

export interface TrackModelChangedArgs {
  oldProvider: string;
  oldModel: string;
  newProvider: string;
  newModel: string;
}

export async function trackModelChanged(args: TrackModelChangedArgs): Promise<void> {
  return invokeTyped<void>('track_model_changed', args);
}

// --- Summaries ---------------------------------------------------------------

export interface TrackSummaryGenerationCompletedArgs {
  modelProvider: string;
  modelName: string;
  success: boolean;
  durationSeconds?: number | null;
  errorMessage?: string | null;
}

export async function trackSummaryGenerationCompleted(
  args: TrackSummaryGenerationCompletedArgs,
): Promise<void> {
  return invokeTyped<void>('track_summary_generation_completed', args);
}

export interface TrackSummaryRegeneratedArgs {
  modelProvider: string;
  modelName: string;
}

export async function trackSummaryRegenerated(args: TrackSummaryRegeneratedArgs): Promise<void> {
  return invokeTyped<void>('track_summary_regenerated', args);
}

export interface TrackCustomPromptUsedArgs {
  promptLength: number;
}

export async function trackCustomPromptUsed(args: TrackCustomPromptUsedArgs): Promise<void> {
  return invokeTyped<void>('track_custom_prompt_used', args);
}
