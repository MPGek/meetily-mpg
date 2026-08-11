export const DIARIZATION_STORAGE_KEYS = {
  enabled: "diarizationEnabled",
  autoRun: "diarizationAutoRun",
  maxSpeakers: "diarizationMaxSpeakers",
} as const;

export interface DiarizationSettings {
  enabled: boolean;
  autoRun: boolean;
  maxSpeakers: number;
}

export function loadDiarizationSettings(): DiarizationSettings {
  if (typeof window === "undefined") {
    return { enabled: false, autoRun: false, maxSpeakers: 0 };
  }

  const enabled = localStorage.getItem(DIARIZATION_STORAGE_KEYS.enabled) === "true";
  const autoRun = localStorage.getItem(DIARIZATION_STORAGE_KEYS.autoRun) === "true";
  const rawMax = localStorage.getItem(DIARIZATION_STORAGE_KEYS.maxSpeakers);
  const maxSpeakers = rawMax !== null ? parseInt(rawMax, 10) : 0;

  return {
    enabled,
    autoRun,
    maxSpeakers: Number.isNaN(maxSpeakers) ? 0 : Math.max(0, Math.min(20, maxSpeakers)),
  };
}

export function saveDiarizationEnabled(enabled: boolean) {
  if (typeof window === "undefined") return;
  localStorage.setItem(DIARIZATION_STORAGE_KEYS.enabled, enabled.toString());
}
