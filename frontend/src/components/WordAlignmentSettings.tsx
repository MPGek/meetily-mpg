"use client";

/**
 * Word-alignment settings section (word-level-diarization-alignment 6.2).
 *
 * Enable toggle + model status/download/cancel/delete, mirroring the
 * transcription engine-selector pattern. Embedded in the diarization panel.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { Switch } from "./ui/switch";
import { Button } from "./ui/button";
import { Label } from "./ui/label";
import {
  alignmentService,
  AlignmentModelInfo,
  AlignmentModelStatus,
} from "@/services/alignmentService";
import {
  loadAlignmentSettings,
  saveAlignmentSettings,
  syncAlignmentSettingsToBackend,
} from "@/lib/alignment";
import { CheckCircle, AlertCircle, Download, Trash2, X } from "lucide-react";

function statusLabel(status: AlignmentModelStatus): string {
  switch (status.state) {
    case "Available":
      return "Ready";
    case "Missing":
      return "Not downloaded";
    case "Downloading":
      return `Downloading ${status.detail.progress}%`;
    case "Corrupted":
      return "Needs re-download";
  }
}

export function WordAlignmentSettings() {
  const [enabled, setEnabled] = useState(() => loadAlignmentSettings().enabled);
  const [modelId, setModelId] = useState(() => loadAlignmentSettings().modelId);
  const [models, setModels] = useState<AlignmentModelInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const mountedRef = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const list = await alignmentService.listModels();
      if (mountedRef.current) setModels(list);
    } catch (err) {
      console.error("Failed to load alignment models:", err);
    } finally {
      if (mountedRef.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    refresh();

    const unsubs: Array<() => void> = [];
    (async () => {
      unsubs.push(
        await alignmentService.onDownloadProgress((p) => {
          setModels((prev) =>
            prev.map((m) =>
              m.id === p.modelId
                ? { ...m, status: { state: "Downloading", detail: { progress: p.progress } } }
                : m
            )
          );
        })
      );
      unsubs.push(
        await alignmentService.onDownloadCompleted(() => {
          refresh();
        })
      );
      unsubs.push(
        await alignmentService.onDownloadFailed(() => {
          refresh();
        })
      );
    })();

    return () => {
      mountedRef.current = false;
      unsubs.forEach((u) => u());
    };
  }, [refresh]);

  const toggleEnabled = (checked: boolean) => {
    setEnabled(checked);
    const settings = { enabled: checked, modelId };
    saveAlignmentSettings(settings);
    syncAlignmentSettingsToBackend(settings);
  };

  const selectModel = (id: string) => {
    setModelId(id);
    const settings = { enabled, modelId: id };
    saveAlignmentSettings(settings);
    syncAlignmentSettingsToBackend(settings);
  };

  const download = async (id: string) => {
    setModels((prev) =>
      prev.map((m) =>
        m.id === id ? { ...m, status: { state: "Downloading", detail: { progress: 0 } } } : m
      )
    );
    try {
      await alignmentService.downloadModel(id);
    } catch (err) {
      console.error("Alignment download failed:", err);
    } finally {
      refresh();
    }
  };

  const cancel = async (id: string) => {
    try {
      await alignmentService.cancelDownload(id);
    } catch (err) {
      console.error("Alignment cancel failed:", err);
    } finally {
      refresh();
    }
  };

  const remove = async (id: string) => {
    try {
      await alignmentService.deleteModel(id);
    } catch (err) {
      console.error("Alignment delete failed:", err);
    } finally {
      refresh();
    }
  };

  return (
    <div className="p-4 border rounded-lg bg-gray-50 space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <Label className="text-sm font-medium text-gray-900">Word alignment</Label>
          <p className="text-xs text-gray-500">
            Refine per-word timestamps with a CTC aligner during recording for precise speaker
            splits. Falls back to engine timestamps when off or unavailable.
          </p>
        </div>
        <Switch checked={enabled} onCheckedChange={toggleEnabled} />
      </div>

      <div className="space-y-2">
        {loading && <p className="text-xs text-gray-400">Loading alignment models…</p>}
        {models.map((model) => {
          const ready = model.status.state === "Available";
          const downloading = model.status.state === "Downloading";
          const selected = model.id === modelId;
          return (
            <div
              key={model.id}
              className={`p-3 rounded-md border bg-white ${
                selected ? "border-blue-400 ring-1 ring-blue-200" : "border-gray-200"
              }`}
            >
              <div className="flex items-center justify-between gap-2">
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-gray-900 truncate">
                      {model.name}
                    </span>
                    {ready ? (
                      <span className="flex items-center gap-1 text-xs font-medium text-green-600">
                        <CheckCircle className="w-3.5 h-3.5" />
                        {statusLabel(model.status)}
                      </span>
                    ) : downloading ? (
                      <span className="text-xs font-medium text-blue-600">
                        {statusLabel(model.status)}
                      </span>
                    ) : (
                      <span className="flex items-center gap-1 text-xs font-medium text-amber-600">
                        <AlertCircle className="w-3.5 h-3.5" />
                        {statusLabel(model.status)}
                      </span>
                    )}
                  </div>
                  <p className="text-xs text-gray-500 truncate">
                    {model.languages} · {model.size_mb} MB
                  </p>
                </div>
                <div className="flex items-center gap-2 shrink-0">
                  {ready ? (
                    <>
                      {!selected && (
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => selectModel(model.id)}
                        >
                          Select
                        </Button>
                      )}
                      <Button size="sm" variant="ghost" onClick={() => remove(model.id)}>
                        <Trash2 className="w-3.5 h-3.5" />
                      </Button>
                    </>
                  ) : downloading ? (
                    <Button size="sm" variant="outline" onClick={() => cancel(model.id)}>
                      <X className="w-3.5 h-3.5 mr-1" />
                      Cancel
                    </Button>
                  ) : (
                    <Button size="sm" onClick={() => download(model.id)}>
                      <Download className="w-3.5 h-3.5 mr-1" />
                      Download
                    </Button>
                  )}
                </div>
              </div>
              {downloading && (
                <div className="mt-2 h-1.5 w-full rounded bg-gray-200 overflow-hidden">
                  <div
                    className="h-full bg-blue-500 transition-all"
                    style={{
                      width:
                        model.status.state === "Downloading"
                          ? `${model.status.detail.progress}%`
                          : "0%",
                    }}
                  />
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
