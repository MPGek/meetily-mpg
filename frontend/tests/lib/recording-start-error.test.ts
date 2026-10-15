import { describe, expect, mock, test } from "bun:test";

mock.module("@tauri-apps/api/core", () => ({ invoke: mock(async () => undefined) }));
mock.module("@tauri-apps/api/event", () => ({ listen: mock(async () => () => {}), emit: mock(async () => {}) }));

const { IpcError } = await import("../../src/lib/ipc/core");
const { formatRecordingStartError } = await import("../../src/lib/recording-start-error");

describe("formatRecordingStartError", () => {
  test("carries an IpcError's backend message", () => {
    const error = new IpcError(
      "start_recording",
      "Failed to start recording: Failed to initialize voice activity detection for microphone: model missing",
      "raw",
    );
    expect(formatRecordingStartError(error)).toBe(
      "Failed to start recording.\n\nFailed to start recording: Failed to initialize voice activity detection for microphone: model missing",
    );
  });

  test("uses a plain string rejection as the message", () => {
    expect(formatRecordingStartError("No microphone found")).toBe(
      "Failed to start recording.\n\nNo microphone found",
    );
  });

  test("uses an Error's message", () => {
    expect(formatRecordingStartError(new Error("device busy"))).toBe(
      "Failed to start recording.\n\ndevice busy",
    );
  });

  test("falls back to the bare sentence when the message is empty", () => {
    expect(formatRecordingStartError("")).toBe("Failed to start recording.");
    expect(formatRecordingStartError(new Error("   "))).toBe("Failed to start recording.");
  });
});
