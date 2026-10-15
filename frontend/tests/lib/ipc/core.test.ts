import { beforeEach, describe, expect, mock, test } from "bun:test";

const invoke = mock(async (_cmd: string, _args?: unknown): Promise<unknown> => undefined);
mock.module("@tauri-apps/api/core", () => ({ invoke }));
mock.module("@tauri-apps/api/event", () => ({ listen: mock(async () => () => {}), emit: mock(async () => {}) }));

const { IpcError, invokeTyped, normalizeMessage } = await import("../../../src/lib/ipc/core");

async function rejection(p: Promise<unknown>): Promise<InstanceType<typeof IpcError>> {
  return p.then(
    () => {
      throw new Error("expected a rejection");
    },
    (e) => e,
  );
}

beforeEach(() => {
  invoke.mockReset();
});

describe("invokeTyped", () => {
  test("a rejected string becomes an IpcError carrying the command", async () => {
    invoke.mockImplementation(async () => {
      throw "Meeting not found";
    });
    const err = await rejection(invokeTyped("api_get_meeting", { meetingId: "m1" }));
    expect(err).toBeInstanceOf(IpcError);
    expect(err).toBeInstanceOf(Error);
    expect(err.message).toBe("Meeting not found");
    expect(err.command).toBe("api_get_meeting");
    expect(err.cause).toBe("Meeting not found");
    expect(String(err)).toBe("Meeting not found");
  });

  test("a rejected Error keeps its message", async () => {
    const original = new Error("boom");
    invoke.mockImplementation(async () => {
      throw original;
    });
    const err = await rejection(invokeTyped("is_recording"));
    expect(err).toBeInstanceOf(IpcError);
    expect(err.message).toBe("boom");
    expect(err.cause).toBe(original);
  });

  test("a successful call resolves with the value unchanged", async () => {
    const value = { a: 1 };
    invoke.mockImplementation(async () => value);
    await expect(invokeTyped("get_x", { id: 1 })).resolves.toBe(value);
    expect(invoke).toHaveBeenCalledWith("get_x", { id: 1 });
  });
});

describe("normalizeMessage", () => {
  test("falls back to String() for other values", () => {
    expect(normalizeMessage(42)).toBe("42");
    expect(normalizeMessage(undefined)).toBe("undefined");
  });
});
