/**
 * Typed wrappers for the onboarding commands (src-tauri/src/onboarding.rs)
 * and the first-launch check (src-tauri/src/database/commands.rs).
 */
import { invokeTyped } from './core';

export interface OnboardingStatus {
  version: string;
  completed: boolean;
  current_step: number;
  model_status: {
    parakeet: string; // "downloaded" | "not_downloaded" | "downloading"
    summary: string;
    selected_summary_model?: string;
  };
  last_updated: string;
}

/** True when no database exists yet. */
export async function checkFirstLaunch(): Promise<boolean> {
  return invokeTyped<boolean>('check_first_launch');
}

export interface CompleteOnboardingArgs {
  model: string;
}

/** Saves the builtin-ai summary model and Parakeet transcript config, then marks onboarding complete. */
export async function completeOnboarding(args: CompleteOnboardingArgs): Promise<void> {
  return invokeTyped<void>('complete_onboarding', args);
}

/** Null when no status has been saved yet. */
export async function getOnboardingStatus(): Promise<OnboardingStatus | null> {
  return invokeTyped<OnboardingStatus | null>('get_onboarding_status');
}

export interface SaveOnboardingStatusCmdArgs {
  status: OnboardingStatus;
}

/** Rust overwrites `last_updated` with the save time. */
export async function saveOnboardingStatusCmd(args: SaveOnboardingStatusCmdArgs): Promise<void> {
  return invokeTyped<void>('save_onboarding_status_cmd', args);
}
