"use client";

import { useEffect, useState, useRef } from "react";
import { Switch } from "./ui/switch";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { recordingService } from "@/services/recordingService";
import { WordAlignmentSettings } from "./WordAlignmentSettings";
import { CheckCircle, AlertCircle } from "lucide-react";
import type { DiarizationMode } from "@/lib/diarization";

const STORAGE_KEYS = {
  enabled: "diarizationEnabled",
  autoRun: "diarizationAutoRun",
  maxSpeakers: "diarizationMaxSpeakers",
  mode: "diarizationMode",
};

export interface DiarizationSettingsState {
  enabled: boolean;
  autoRun: boolean;
  maxSpeakers: number;
  diarizationMode: DiarizationMode;
}

function loadBoolean(key: string, defaultValue: boolean): boolean {
  if (typeof window === "undefined") return defaultValue;
  const raw = localStorage.getItem(key);
  return raw !== null ? raw === "true" : defaultValue;
}

function loadNumber(key: string, defaultValue: number): number {
  if (typeof window === "undefined") return defaultValue;
  const raw = localStorage.getItem(key);
  if (raw === null) return defaultValue;
  const parsed = parseInt(raw, 10);
  return Number.isNaN(parsed) ? defaultValue : parsed;
}

function loadMode(defaultValue: DiarizationMode): DiarizationMode {
  if (typeof window === "undefined") return defaultValue;
  const raw = localStorage.getItem(STORAGE_KEYS.mode);
  return raw === "off" || raw === "efficient" || raw === "fast" ? raw : defaultValue;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(1)} GB`;
}

interface SpeakerStorageStats {
  registry_count: number;
  prototype_count: number;
  cache_count: number;
  total_bytes: number;
}

interface DiarizationModelStatus {
  segmentation_ready: boolean;
  embedding_ready: boolean;
  ready: boolean;
}

function saveSetting(key: string, value: string) {
  if (typeof window === "undefined") return;
  localStorage.setItem(key, value);
}

export function DiarizationSettings() {
  const [enabled, setEnabled] = useState(() => loadBoolean(STORAGE_KEYS.enabled, false));
  const [autoRun, setAutoRun] = useState(() => loadBoolean(STORAGE_KEYS.autoRun, false));
  const [maxSpeakers, setMaxSpeakers] = useState(() => loadNumber(STORAGE_KEYS.maxSpeakers, 0));
  const [diarizationMode, setDiarizationMode] = useState<DiarizationMode>(() =>
    loadMode("efficient")
  );
  const [modelsReady, setModelsReady] = useState<DiarizationModelStatus | null>(null);
  const [speakerStats, setSpeakerStats] = useState<SpeakerStorageStats | null>(null);

  const mountedRef = useRef(false);

  const loadSpeakerStats = async () => {
    try {
      const stats = await recordingService.speakerStorageStats();
      if (mountedRef.current) setSpeakerStats(stats);
    } catch (error) {
      console.error("Failed to load speaker storage stats:", error);
    }
  };

  const checkModels = async () => {
    try {
      const status = await recordingService.checkDiarizationModels();
      if (mountedRef.current) {
        setModelsReady({
          segmentation_ready: status.segmentation_ready,
          embedding_ready: status.embedding_ready,
          ready: status.ready,
        });
      }
    } catch (error) {
      console.error("Failed to check diarization models:", error);
    }
  };

  useEffect(() => {
    mountedRef.current = true;
    checkModels();
    loadSpeakerStats();

    return () => {
      mountedRef.current = false;
    };
  }, []);

  return (
    <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm space-y-6">
      <div>
        <h3 className="text-lg font-semibold text-gray-900 mb-2">Speaker Diarization</h3>
        <p className="text-sm text-gray-600">
          Automatically identify who spoke when in your meetings using local ONNX models.
        </p>
      </div>

      {/* Enable toggle */}
      <div className="flex items-center justify-between">
        <div>
          <Label className="text-sm font-medium text-gray-900">Enable speaker diarization</Label>
          <p className="text-xs text-gray-500">Run speaker analysis after recordings and on past meetings.</p>
        </div>
        <Switch
          checked={enabled}
          onCheckedChange={(checked) => {
            setEnabled(checked);
            saveSetting(STORAGE_KEYS.enabled, checked.toString());
          }}
        />
      </div>

      {/* Auto-run toggle */}
      <div className="flex items-center justify-between">
        <div>
          <Label className="text-sm font-medium text-gray-900">Auto-run after recording</Label>
          <p className="text-xs text-gray-500">Automatically analyze speakers when a recording stops.</p>
        </div>
        <Switch
          checked={autoRun}
          disabled={!enabled}
          onCheckedChange={(checked) => {
            setAutoRun(checked);
            saveSetting(STORAGE_KEYS.autoRun, checked.toString());
          }}
        />
      </div>

      {/* Diarization mode */}
      <div>
        <Label className="text-sm font-medium text-gray-900 mb-1 block">Diarization mode</Label>
        <select
          disabled={!enabled}
          value={diarizationMode}
          onChange={(e) => {
            const mode = e.target.value as DiarizationMode;
            setDiarizationMode(mode);
            saveSetting(STORAGE_KEYS.mode, mode);
          }}
          className="w-full sm:w-64 rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-900 shadow-sm focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:opacity-50"
        >
          <option value="efficient">Efficient (recommended)</option>
          <option value="fast">Fast</option>
          <option value="off">Off</option>
        </select>
        <p className="text-xs text-gray-500 mt-2">
          {diarizationMode === "efficient" &&
            "Extracts speaker embeddings during recording and labels speakers at the end. Low CPU usage, labels appear as soon as the recording stops."}
          {diarizationMode === "fast" &&
            "Runs full streaming diarization during recording for the highest accuracy. Higher CPU usage during the meeting."}
          {diarizationMode === "off" &&
            "No diarization during recording. Use “Re-analyze Speakers” after the meeting to run offline speaker analysis."}
        </p>
      </div>

      {/* Max speakers */}
      <div>
        <Label className="text-sm font-medium text-gray-900 mb-1 block">Max speakers</Label>
        <p className="text-xs text-gray-500 mb-2">Set to 0 to let the model auto-detect the number of speakers.</p>
        <Input
          type="number"
          min={0}
          max={20}
          disabled={!enabled}
          value={maxSpeakers}
          onChange={(e) => {
            const value = parseInt(e.target.value, 10);
            const normalized = Number.isNaN(value) ? 0 : Math.max(0, Math.min(20, value));
            setMaxSpeakers(normalized);
            saveSetting(STORAGE_KEYS.maxSpeakers, normalized.toString());
          }}
          className="w-32"
        />
      </div>

      {/* Enhanced model set - read-only, bundled at build time */}
      <div className="p-4 border rounded-lg bg-gray-50">
        <div className="flex items-center justify-between mb-3">
          <div className="font-medium">Enhanced Models (segmentation-3.0 + TitaNet-Large)</div>
          {modelsReady?.ready ? (
            <span className="flex items-center gap-1 text-xs font-medium text-green-600">
              <CheckCircle className="w-3.5 h-3.5" />
              Ready (bundled)
            </span>
          ) : (
            <span className="flex items-center gap-1 text-xs font-medium text-amber-600">
              <AlertCircle className="w-3.5 h-3.5" />
              {modelsReady ? "Not bundled" : "Checking…"}
            </span>
          )}
        </div>
        {modelsReady && (
          <div className="space-y-1 text-xs text-gray-600 mb-3">
            <div className="flex items-center gap-2">
              <span className={modelsReady.segmentation_ready ? "text-green-600" : "text-amber-600"}>
                {modelsReady.segmentation_ready ? "✓" : "○"}
              </span>
              <span>Enhanced segmentation-3.0 (onnx-community)</span>
            </div>
            <div className="flex items-center gap-2">
              <span className={modelsReady.embedding_ready ? "text-green-600" : "text-amber-600"}>
                {modelsReady.embedding_ready ? "✓" : "○"}
              </span>
              <span>Enhanced TitaNet-Large (192-d, Recogment)</span>
            </div>
          </div>
        )}
        <p className="text-xs text-gray-500 mt-2">
          Bundled at build time from public Hugging Face (onnx-community + Recogment), both files required. Models are
          resolved from AppData → bundled resources (near executable) → dev manifest; diarization fails with an error
          listing all searched locations when none is found. No runtime download — rebuild with network to update the
          bundle.
        </p>
      </div>

      {/* Word-level CTC alignment (live refinement + repair) */}
      <WordAlignmentSettings />

      {/* Voiceprint storage stats */}
      <div className="p-4 border rounded-lg bg-gray-50">
        <div className="flex items-center justify-between mb-2">
          <div className="font-medium">Voiceprint Storage</div>          {speakerStats && (
            <span className="text-xs text-gray-500">{formatBytes(speakerStats.total_bytes)}</span>
          )}
        </div>
        <p className="text-xs text-gray-500 mb-3">
          Known voices and per-meeting voiceprint caches are retained indefinitely.
        </p>
        {speakerStats ? (
          <div className="grid grid-cols-3 gap-3 text-center">
            <div>
              <div className="text-lg font-semibold text-gray-900">{speakerStats.registry_count}</div>
              <div className="text-xs text-gray-500">Known speakers</div>
            </div>
            <div>
              <div className="text-lg font-semibold text-gray-900">{speakerStats.prototype_count}</div>
              <div className="text-xs text-gray-500">Voiceprints</div>
            </div>
            <div>
              <div className="text-lg font-semibold text-gray-900">{speakerStats.cache_count}</div>
              <div className="text-xs text-gray-500">Meeting caches</div>
            </div>
          </div>
        ) : (
          <p className="text-xs text-gray-400">Loading voiceprint stats...</p>
        )}
      </div>
    </div>
  );
}