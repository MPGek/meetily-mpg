import { beforeEach, describe, expect, mock, test } from "bun:test";

const invoke = mock(async (_cmd: string, _args?: unknown): Promise<unknown> => undefined);
mock.module("@tauri-apps/api/core", () => ({ invoke }));
mock.module("@tauri-apps/api/event", () => ({ listen: mock(async () => () => {}), emit: mock(async () => {}) }));

const analytics = await import("../../../src/lib/ipc/analytics");

beforeEach(() => {
  invoke.mockReset();
});

describe("lib/ipc/analytics", () => {
  test("startAnalyticsSession sends the command and args and returns the session id", async () => {
    invoke.mockImplementation(async () => "session-1");
    await expect(analytics.startAnalyticsSession({ userId: "u1" })).resolves.toBe("session-1");
    expect(invoke).toHaveBeenCalledWith("start_analytics_session", { userId: "u1" });
  });

  test("trackEvent forwards the event name and properties unchanged", async () => {
    await analytics.trackEvent({ eventName: "x", properties: { a: "1" } });
    expect(invoke).toHaveBeenCalledWith("track_event", { eventName: "x", properties: { a: "1" } });
  });
});
