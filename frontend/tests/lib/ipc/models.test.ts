import { beforeEach, describe, expect, mock, test } from "bun:test";

const invoke = mock(async (_cmd: string, _args?: unknown): Promise<unknown> => undefined);
mock.module("@tauri-apps/api/core", () => ({ invoke }));
mock.module("@tauri-apps/api/event", () => ({ listen: mock(async () => () => {}), emit: mock(async () => {}) }));

const models = await import("../../../src/lib/ipc/models");

beforeEach(() => {
  invoke.mockReset();
});

describe("lib/ipc/models", () => {
  test("parakeetCancelDownload passes the cancelled/pending outcome through", async () => {
    for (const outcome of ["cancelled", "pending"] as const) {
      invoke.mockImplementation(async () => outcome);
      await expect(models.parakeetCancelDownload({ modelName: "parakeet-tdt-0.6b-v3-int8" })).resolves.toBe(outcome);
      expect(invoke).toHaveBeenLastCalledWith("parakeet_cancel_download", { modelName: "parakeet-tdt-0.6b-v3-int8" });
    }
  });

  test("whisperCancelDownload passes the cancelled/pending outcome through", async () => {
    for (const outcome of ["cancelled", "pending"] as const) {
      invoke.mockImplementation(async () => outcome);
      await expect(models.whisperCancelDownload({ modelName: "base" })).resolves.toBe(outcome);
      expect(invoke).toHaveBeenLastCalledWith("whisper_cancel_download", { modelName: "base" });
    }
  });

  test("cancelAlignmentDownload passes the outcome through", async () => {
    for (const outcome of ["cancelled", "pending"] as const) {
      invoke.mockImplementation(async () => outcome);
      await expect(models.cancelAlignmentDownload({ modelId: "wav2vec2-xlsr-56" })).resolves.toBe(outcome);
      expect(invoke).toHaveBeenLastCalledWith("cancel_alignment_download", { modelId: "wav2vec2-xlsr-56" });
    }
  });
});
