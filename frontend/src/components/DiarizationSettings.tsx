"use client";

import { useEffect, useState, useRef } from "react";
import { Switch } from "./ui/switch";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { recordingService } from "@/services/recordingService";
import { toast } from "sonner";
import { Download, CheckCircle, AlertCircle, Loader2 } from "lucide-react";
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
  const [modelsReady, setModelsReady] = useState<{ segmentation: boolean; embedding: boolean } | null>(null);
  const [isDownloading, setIsDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [downloadMessage, setDownloadMessage] = useState("");

  const mountedRef = useRef(false);

  const checkModels = async () => {
    try {
      const status = await recordingService.checkDiarizationModels();
      setModelsReady({
        segmentation: status.segmentation_ready,
        embedding: status.embedding_ready,
      });
    } catch (error) {
      console.error("Failed to check diarization models:", error);
    }
  };

  useEffect(() => {
    mountedRef.current = true;
    checkModels();

    let unlistenProgress: (() => void) | undefined;
    let unlistenComplete: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;

    const setupListeners = async () => {
      unlistenProgress = await recordingService.onDiarizationModelDownloadProgress((progress, message) => {
        if (!mountedRef.current) return;
        setDownloadProgress(progress);
        setDownloadMessage(message);
      });

      unlistenComplete = await recordingService.onDiarizationModelDownloadComplete(() => {
        if (!mountedRef.current) return;
        setIsDownloading(false);
        setDownloadProgress(100);
        setDownloadMessage("Models ready");
        toast.success("Diarization models downloaded", {
          description: "Speaker analysis is now available.",
        });
        checkModels();
      });

      unlistenError = await recordingService.onDiarizationModelDownloadError((error) => {
        if (!mountedRef.current) return;
        setIsDownloading(false);
        setDownloadProgress(0);
        setDownloadMessage("");
        toast.error("Failed to download diarization models", {
          description: error,
          action: {
            label: "Retry",
            onClick: handleDownload,
          },
        });
      });
    };

    setupListeners();

    return () => {
      mountedRef.current = false;
      if (unlistenProgress) unlistenProgress();
      if (unlistenComplete) unlistenComplete();
      if (unlistenError) unlistenError();
    };
  }, []);

  const handleDownload = async () => {
    if (isDownloading) return;
    setIsDownloading(true);
    setDownloadProgress(0);
    setDownloadMessage("Starting download...");
    try {
      await recordingService.downloadDiarizationModels();
    } catch (error) {
      setIsDownloading(false);
      setDownloadProgress(0);
      setDownloadMessage("");
      toast.error("Failed to start model download", {
        description: error instanceof Error ? error.message : "Unknown error",
      });
    }
  };

  const allModelsReady = modelsReady?.segmentation && modelsReady?.embedding;
  const anyModelMissing = modelsReady && !allModelsReady;

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

      {/* Model status and download */}
      <div className="p-4 border rounded-lg bg-gray-50">
        <div className="flex items-center justify-between mb-3">
          <div className="font-medium">Diarization Models</div>
          {allModelsReady ? (
            <span className="flex items-center gap-1 text-xs font-medium text-green-600">
              <CheckCircle className="w-3.5 h-3.5" />
              Ready
            </span>
          ) : (
            <span className="flex items-center gap-1 text-xs font-medium text-amber-600">
              <AlertCircle className="w-3.5 h-3.5" />
              Not downloaded
            </span>
          )}
        </div>

        {modelsReady && (
          <div className="space-y-1 text-xs text-gray-600 mb-3">
            <div className="flex items-center gap-2">
              <span className={modelsReady.segmentation ? "text-green-600" : "text-amber-600"}>
                {modelsReady.segmentation ? "✓" : "○"}
              </span>
              <span>Segmentation model</span>
            </div>
            <div className="flex items-center gap-2">
              <span className={modelsReady.embedding ? "text-green-600" : "text-amber-600"}>
                {modelsReady.embedding ? "✓" : "○"}
              </span>
              <span>Speaker embedding model</span>
            </div>
          </div>
        )}

        {isDownloading ? (
          <div className="space-y-2">
            <div className="flex items-center justify-between text-xs">
              <span className="text-blue-700 font-medium">{downloadMessage}</span>
              <span className="text-blue-700 font-semibold">{Math.round(downloadProgress)}%</span>
            </div>
            <div className="w-full h-2 bg-gray-200 rounded-full overflow-hidden">
              <div
                className="h-full bg-blue-600 rounded-full transition-all duration-300"
                style={{ width: `${downloadProgress}%` }}
              />
            </div>
          </div>
        ) : (
          <Button
            onClick={handleDownload}
            disabled={isDownloading}
            variant={allModelsReady ? "outline" : "default"}
            className="w-full sm:w-auto"
          >
            {allModelsReady ? (
              <>
                <Download className="w-4 h-4 mr-2" />
                Re-download Models
              </>
            ) : (
              <>
                {anyModelMissing ? <AlertCircle className="w-4 h-4 mr-2" /> : <Download className="w-4 h-4 mr-2" />}
                Download Models
              </>
            )}
          </Button>
        )}

        <p className="text-xs text-gray-500 mt-3">
          Models are downloaded to your application data directory and run entirely on-device.
        </p>
      </div>
    </div>
  );
}
