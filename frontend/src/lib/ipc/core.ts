/**
 * The single Tauri IPC boundary. Every `invoke`/`listen` in the app goes
 * through this file (enforced by `no-restricted-imports` in .eslintrc.json);
 * domain modules in this folder wrap individual commands and events.
 */
import { invoke, type InvokeArgs } from '@tauri-apps/api/core';
import { emit, listen, type EventCallback, type UnlistenFn } from '@tauri-apps/api/event';

export type { Event, EventCallback, UnlistenFn } from '@tauri-apps/api/event';

/**
 * A failed Tauri command. Rust commands return `Result<T, String>`, so the
 * original rejection is usually a plain string; it is kept as `cause`.
 */
export class IpcError extends Error {
  readonly command: string;
  readonly cause: unknown;

  constructor(command: string, message: string, cause: unknown) {
    super(message);
    this.name = 'IpcError';
    this.command = command;
    this.cause = cause;
  }

  // Call sites that format the rejection with String(err) or `${err}` keep
  // getting the bare message, as they did when the rejection was a string.
  toString(): string {
    return this.message;
  }
}

export function normalizeMessage(reason: unknown): string {
  if (typeof reason === 'string') return reason;
  if (reason instanceof Error) return reason.message;
  return String(reason);
}

let ipcDebug = false;

/** Turns on `console.debug('[ipc]', ...)` logging for every command. Off by default. */
export function setIpcDebug(enabled: boolean): void {
  ipcDebug = enabled;
}

export async function invokeTyped<TResult, TArgs extends object = object>(
  cmd: string,
  args?: TArgs,
  opts?: { debug?: boolean },
): Promise<TResult> {
  const debug = opts?.debug ?? ipcDebug;
  try {
    const result = await invoke<TResult>(cmd, args as InvokeArgs | undefined);
    if (debug) console.debug('[ipc]', cmd, 'ok');
    return result;
  } catch (reason) {
    if (debug) console.debug('[ipc]', cmd, 'failed', reason);
    throw new IpcError(cmd, normalizeMessage(reason), reason);
  }
}

export function listenTyped<TPayload>(
  event: string,
  handler: EventCallback<TPayload>,
): Promise<UnlistenFn> {
  return listen<TPayload>(event, handler);
}

/** Emits a frontend-originated app event (e.g. `model-config-updated`). */
export function emitTyped<TPayload>(event: string, payload: TPayload): Promise<void> {
  return emit(event, payload);
}
