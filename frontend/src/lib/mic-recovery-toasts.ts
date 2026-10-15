/**
 * Toast copy for the backend's microphone recovery events
 * (mic-disconnect-recovery). Pure, so the wording is unit-tested; the
 * RecordingStateProvider shows the result with a fixed toast id so each
 * notification replaces the previous one.
 */
import type {
  MicDeviceSwitchedPayload,
  MicRecoveryExhaustedPayload,
  MicSwapFailedPayload,
} from './ipc/recording';

export type MicRecoveryToastLevel = 'info' | 'warning' | 'error';

export interface MicRecoveryToast {
  level: MicRecoveryToastLevel;
  title: string;
  description: string;
}

export type MicRecoveryKind = 'switched' | 'failed' | 'exhausted';

export function micRecoveryToast(kind: 'switched', payload: MicDeviceSwitchedPayload): MicRecoveryToast;
export function micRecoveryToast(kind: 'failed', payload: MicSwapFailedPayload): MicRecoveryToast;
export function micRecoveryToast(kind: 'exhausted', payload: MicRecoveryExhaustedPayload): MicRecoveryToast;
export function micRecoveryToast(
  kind: MicRecoveryKind,
  payload: MicDeviceSwitchedPayload | MicSwapFailedPayload | MicRecoveryExhaustedPayload,
): MicRecoveryToast {
  switch (kind) {
    case 'switched': {
      const p = payload as MicDeviceSwitchedPayload;
      if (p.reason === 'unavailable_at_start') {
        return {
          level: 'info',
          title: 'Microphone switched',
          description: `Selected microphone unavailable ("${p.previous_device_name}"). Recording on "${p.device_name}" instead.`,
        };
      }
      return {
        level: 'info',
        title: 'Microphone switched',
        description: `"${p.previous_device_name}" was disconnected. Recording continues on "${p.device_name}" for this meeting.`,
      };
    }
    case 'failed': {
      const p = payload as MicSwapFailedPayload;
      return {
        level: 'warning',
        title: 'Microphone lost',
        description: `"${p.device_name}" was disconnected and switching to another microphone failed (${p.error}); retrying (${p.attempt}/${p.max_attempts}).`,
      };
    }
    case 'exhausted': {
      const p = payload as MicRecoveryExhaustedPayload;
      return {
        level: 'error',
        title: 'Microphone could not be recovered',
        description: `"${p.device_name}" was lost. The recording continues without a microphone; stop and restart the recording to use another one.`,
      };
    }
  }
}
