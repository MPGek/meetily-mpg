import { describe, expect, test } from "bun:test";

import { micRecoveryToast } from "../../src/lib/mic-recovery-toasts";

describe("micRecoveryToast", () => {
  test("a mid-recording switch is info naming both devices, for this meeting", () => {
    const toast = micRecoveryToast("switched", {
      device_name: "Microphone Array (Realtek)",
      previous_device_name: "USB Audio Mic",
      reason: "disconnected",
    });
    expect(toast.level).toBe("info");
    expect(toast.title).toBe("Microphone switched");
    expect(toast.description).toContain("Microphone Array (Realtek)");
    expect(toast.description).toContain("USB Audio Mic");
    expect(toast.description).toContain("for this meeting");
  });

  test("a start-time fallback says the selected microphone was unavailable", () => {
    const toast = micRecoveryToast("switched", {
      device_name: "Microphone Array (Realtek)",
      previous_device_name: "Jabra Evolve 75",
      reason: "unavailable_at_start",
    });
    expect(toast.level).toBe("info");
    expect(toast.description).toContain("Selected microphone unavailable");
    expect(toast.description).toContain("Jabra Evolve 75");
    expect(toast.description).toContain("Microphone Array (Realtek)");
  });

  test("a failed attempt is a warning with the retry count", () => {
    const toast = micRecoveryToast("failed", {
      device_name: "USB Audio Mic",
      error: "no default input device",
      attempt: 1,
      max_attempts: 3,
    });
    expect(toast.level).toBe("warning");
    expect(toast.description).toContain("USB Audio Mic");
    expect(toast.description).toContain("retrying (1/3)");
  });

  test("exhaustion is an error: no microphone, stop and restart", () => {
    const toast = micRecoveryToast("exhausted", { device_name: "USB Audio Mic" });
    expect(toast.level).toBe("error");
    expect(toast.title).toBe("Microphone could not be recovered");
    expect(toast.description).toContain("USB Audio Mic");
    expect(toast.description).toContain("continues without a microphone");
    expect(toast.description).toContain("stop and restart");
  });
});
