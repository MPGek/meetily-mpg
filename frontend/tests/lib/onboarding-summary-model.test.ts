import { describe, expect, test } from "bun:test";
import {
  getDownloadTotalMb,
  getSummaryModelSizeLabel,
  getSummaryModelSizeMb,
  resolveOnboardingSummaryModelStatus,
} from "../../src/lib/onboarding-summary-model";

describe("resolveOnboardingSummaryModelStatus", () => {
  test("legacy Gemma availability must not make an undownloaded selected Qwen model ready", () => {
    expect(
      resolveOnboardingSummaryModelStatus({
        selectedModel: "qwen3.5:4b",
        recommendedModel: "qwen3.5:4b",
        selectedModelReady: false,
      })
    ).toEqual({
      selectedSummaryModel: "qwen3.5:4b",
      summaryModelDownloaded: false,
    });
  });

  test("explicit selected model should win over a different recommendation", () => {
    expect(
      resolveOnboardingSummaryModelStatus({
        selectedModel: "gemma3:1b",
        recommendedModel: "qwen3.5:4b",
        selectedModelReady: true,
      })
    ).toEqual({
      selectedSummaryModel: "gemma3:1b",
      summaryModelDownloaded: true,
    });
  });

  test("recommended Qwen should become the selected model when no model is selected yet", () => {
    expect(
      resolveOnboardingSummaryModelStatus({
        selectedModel: "",
        recommendedModel: "qwen3.5:2b",
        selectedModelReady: true,
      })
    ).toEqual({
      selectedSummaryModel: "qwen3.5:2b",
      summaryModelDownloaded: true,
    });
  });
});

describe("getSummaryModelSizeMb", () => {
  test("returns known model sizes and 0 for an unknown model", () => {
    expect(getSummaryModelSizeMb("qwen3.5:2b")).toBe(1221);
    expect(getSummaryModelSizeMb("qwen3.5:4b")).toBe(2614);
    expect(getSummaryModelSizeMb("gemma3:1b")).toBe(1019);
    expect(getSummaryModelSizeMb("unknown:model")).toBe(0);
  });
});

describe("getSummaryModelSizeLabel", () => {
  test("formats known model sizes and returns an empty label for an unknown model", () => {
    expect(getSummaryModelSizeLabel("qwen3.5:2b")).toBe("~1.2 GiB");
    expect(getSummaryModelSizeLabel("qwen3.5:4b")).toBe("~2.6 GiB");
    expect(getSummaryModelSizeLabel("unknown:model")).toBe("");
  });
});

describe("getDownloadTotalMb", () => {
  test("falls back to the model's known size when no total is reported yet", () => {
    expect(getDownloadTotalMb(0, "qwen3.5:4b")).toBe(2614);
    expect(getDownloadTotalMb(undefined, "qwen3.5:2b")).toBe(1221);
    expect(getDownloadTotalMb(512, "qwen3.5:4b")).toBe(512);
  });
});
