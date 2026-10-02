import { beforeEach, describe, expect, mock, test } from "bun:test";

const unlisten = () => {};
const listen = mock(async (_event: string, _handler: (event: { payload: unknown }) => void) => unlisten);
mock.module("@tauri-apps/api/core", () => ({ invoke: mock(async () => undefined) }));
mock.module("@tauri-apps/api/event", () => ({ listen, emit: mock(async () => {}) }));

const { listenMicDeviceSwitched, listenMicRecoveryExhausted, listenMicSwapFailed } = await import(
  "../../../src/lib/ipc/recording"
);

beforeEach(() => {
  listen.mockClear();
});

describe("mic recovery event listeners", () => {
  test("listenMicDeviceSwitched registers mic-device-switched", async () => {
    await expect(listenMicDeviceSwitched(() => {})).resolves.toBe(unlisten);
    expect(listen).toHaveBeenCalledTimes(1);
    expect(listen.mock.calls[0][0]).toBe("mic-device-switched");
  });

  test("listenMicSwapFailed registers mic-swap-failed", async () => {
    await listenMicSwapFailed(() => {});
    expect(listen.mock.calls[0][0]).toBe("mic-swap-failed");
  });

  test("listenMicRecoveryExhausted registers mic-recovery-exhausted", async () => {
    await listenMicRecoveryExhausted(() => {});
    expect(listen.mock.calls[0][0]).toBe("mic-recovery-exhausted");
  });

  test("the handler receives the backend payload unchanged", async () => {
    const seen: unknown[] = [];
    await listenMicSwapFailed((event) => seen.push(event.payload));
    const payload = { device_name: "USB Mic", error: "boom", attempt: 2, max_attempts: 3 };
    listen.mock.calls[0][1]({ payload });
    expect(seen).toEqual([payload]);
  });
});
