export const DIARIZATION_STORAGE_KEYS = {
  enabled: "diarizationEnabled",
  autoRun: "diarizationAutoRun",
  maxSpeakers: "diarizationMaxSpeakers",
  mode: "diarizationMode",
  memoryMode: "diarizationMemoryMode",
  maxSessions: "diarizationMaxSessions",
} as const;

export type DiarizationMode = "off" | "efficient" | "fast";
export type DiarizationMemoryMode = "auto" | "fast" | "low_memory";

export interface DiarizationSettings {
  enabled: boolean;
  autoRun: boolean;
  maxSpeakers: number;
  diarizationMode: DiarizationMode;
  memoryMode: DiarizationMemoryMode;
  maxSessions: number;
}

export function loadDiarizationSettings(): DiarizationSettings {
  if (typeof window === "undefined") {
    return {
      enabled: false,
      autoRun: false,
      maxSpeakers: 0,
      diarizationMode: "efficient",
      memoryMode: "auto",
      maxSessions: 0,
    };
  }

  const enabled = localStorage.getItem(DIARIZATION_STORAGE_KEYS.enabled) === "true";
  const autoRun = localStorage.getItem(DIARIZATION_STORAGE_KEYS.autoRun) === "true";
  const rawMax = localStorage.getItem(DIARIZATION_STORAGE_KEYS.maxSpeakers);
  const maxSpeakers = rawMax !== null ? parseInt(rawMax, 10) : 0;
  const rawMode = localStorage.getItem(DIARIZATION_STORAGE_KEYS.mode);
  const mode: DiarizationMode =
    rawMode === "off" || rawMode === "efficient" || rawMode === "fast" ? rawMode : "efficient";

  const rawMemoryMode = localStorage.getItem(DIARIZATION_STORAGE_KEYS.memoryMode);
  const memoryMode: DiarizationMemoryMode =
    rawMemoryMode === "auto" || rawMemoryMode === "fast" || rawMemoryMode === "low_memory"
      ? rawMemoryMode
      : "auto";

  const rawMaxSessions = localStorage.getItem(DIARIZATION_STORAGE_KEYS.maxSessions);
  const maxSessions = rawMaxSessions !== null ? parseInt(rawMaxSessions, 10) : 0;

  return {
    enabled,
    autoRun,
    maxSpeakers: Number.isNaN(maxSpeakers) ? 0 : Math.max(0, Math.min(20, maxSpeakers)),
    diarizationMode: mode,
    memoryMode,
    maxSessions: Number.isNaN(maxSessions) ? 0 : Math.max(0, Math.min(16, maxSessions)),
  };
}

export function saveDiarizationEnabled(enabled: boolean) {
  if (typeof window === "undefined") return;
  localStorage.setItem(DIARIZATION_STORAGE_KEYS.enabled, enabled.toString());
}

export function saveDiarizationMemorySettings(
  memoryMode: DiarizationMemoryMode,
  maxSessions: number
) {
  if (typeof window === "undefined") return;
  localStorage.setItem(DIARIZATION_STORAGE_KEYS.memoryMode, memoryMode);
  localStorage.setItem(
    DIARIZATION_STORAGE_KEYS.maxSessions,
    Math.max(0, Math.min(16, maxSessions)).toString()
  );
}
