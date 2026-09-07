export const DIARIZATION_STORAGE_KEYS = {
  enabled: "diarizationEnabled",
  autoRun: "diarizationAutoRun",
  maxSpeakers: "diarizationMaxSpeakers",
  mode: "diarizationMode",
} as const;

export type DiarizationMode = "off" | "efficient" | "fast";

export interface DiarizationSettings {
  enabled: boolean;
  autoRun: boolean;
  maxSpeakers: number;
  diarizationMode: DiarizationMode;
}

export function loadDiarizationSettings(): DiarizationSettings {
  if (typeof window === "undefined") {
    return {
      enabled: false,
      autoRun: false,
      maxSpeakers: 0,
      diarizationMode: "efficient",
    };
  }

  const enabled = localStorage.getItem(DIARIZATION_STORAGE_KEYS.enabled) === "true";
  const autoRun = localStorage.getItem(DIARIZATION_STORAGE_KEYS.autoRun) === "true";
  const rawMax = localStorage.getItem(DIARIZATION_STORAGE_KEYS.maxSpeakers);
  const maxSpeakers = rawMax !== null ? parseInt(rawMax, 10) : 0;
  const rawMode = localStorage.getItem(DIARIZATION_STORAGE_KEYS.mode);
  const mode: DiarizationMode =
    rawMode === "off" || rawMode === "efficient" || rawMode === "fast" ? rawMode : "efficient";

  return {
    enabled,
    autoRun,
    maxSpeakers: Number.isNaN(maxSpeakers) ? 0 : Math.max(0, Math.min(20, maxSpeakers)),
    diarizationMode: mode,
  };
}

export function saveDiarizationEnabled(enabled: boolean) {
  if (typeof window === "undefined") return;
  localStorage.setItem(DIARIZATION_STORAGE_KEYS.enabled, enabled.toString());
}

/**
 * Offline clustering parameter overrides (diarization-param-tuning D2).
 * Keys stay unset until a power user writes them; unset means the built-in
 * Rust defaults apply. Mirrored to the backend via
 * `set_diarization_clustering_settings` on startup and after each change.
 */
export type DiarizationClusterer = "vbx" | "nmesc" | "ahc";

export const DIARIZATION_CLUSTERING_KEYS = {
  clusterThreshold: "diarizationClusterThreshold",
  clusterCeiling: "diarizationClusterCeiling",
  gapMergeSecs: "diarizationGapMergeSecs",
  clusterer: "diarizationClusterer",
} as const;

export interface DiarizationClusteringSettings {
  clusterThreshold: number | null;
  clusterCeiling: number | null;
  gapMergeSecs: number | null;
  clusterer: DiarizationClusterer | null;
}

function loadNumberOrNull(key: string): number | null {
  if (typeof window === "undefined") return null;
  const raw = localStorage.getItem(key);
  if (raw === null) return null;
  const value = parseFloat(raw);
  return Number.isFinite(value) ? value : null;
}

function loadClustererOrNull(key: string): DiarizationClusterer | null {
  if (typeof window === "undefined") return null;
  const raw = localStorage.getItem(key);
  return raw === "vbx" || raw === "nmesc" || raw === "ahc" ? raw : null;
}

export function loadClusteringSettings(): DiarizationClusteringSettings {
  return {
    clusterThreshold: loadNumberOrNull(DIARIZATION_CLUSTERING_KEYS.clusterThreshold),
    clusterCeiling: loadNumberOrNull(DIARIZATION_CLUSTERING_KEYS.clusterCeiling),
    gapMergeSecs: loadNumberOrNull(DIARIZATION_CLUSTERING_KEYS.gapMergeSecs),
    clusterer: loadClustererOrNull(DIARIZATION_CLUSTERING_KEYS.clusterer),
  };
}

export function saveClusteringSettings(
  settings: Partial<Omit<DiarizationClusteringSettings, "clusterer">> & {
    clusterer?: DiarizationClusterer | null;
  }
): void {
  if (typeof window === "undefined") return;
  const numericEntries: [keyof Omit<DiarizationClusteringSettings, "clusterer">, string][] = [
    ["clusterThreshold", DIARIZATION_CLUSTERING_KEYS.clusterThreshold],
    ["clusterCeiling", DIARIZATION_CLUSTERING_KEYS.clusterCeiling],
    ["gapMergeSecs", DIARIZATION_CLUSTERING_KEYS.gapMergeSecs],
  ];
  for (const [field, key] of numericEntries) {
    const value = settings[field];
    if (value === undefined) continue; // field not addressed by this update
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, String(value));
  }
  if (settings.clusterer !== undefined) {
    if (settings.clusterer === null) localStorage.removeItem(DIARIZATION_CLUSTERING_KEYS.clusterer);
    else localStorage.setItem(DIARIZATION_CLUSTERING_KEYS.clusterer, settings.clusterer);
  }
}

/** Push the persisted clustering overrides to the Rust backend (best-effort). */
export async function syncClusteringSettingsToBackend(
  settings: DiarizationClusteringSettings = loadClusteringSettings()
): Promise<void> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("set_diarization_clustering_settings", {
      clusterThreshold: settings.clusterThreshold,
      clusterCeiling: settings.clusterCeiling,
      gapMergeSecs: settings.gapMergeSecs,
      clusterer: settings.clusterer,
    });
  } catch (err) {
    console.error("[diarization] Failed to sync clustering settings to backend:", err);
  }
}
