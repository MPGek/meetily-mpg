import { describe, expect, test } from "bun:test";
import { sourceSideLayout } from "../../src/lib/source-side-layout";

describe("sourceSideLayout", () => {
    test("System rows render on the right with an emerald surface and the dot after the label", () => {
        const side = sourceSideLayout("System");
        expect(side.isSystem).toBe(true);
        expect(side.isMic).toBe(false);
        expect(side.dotPosition).toBe("after");
        expect(side.labelRowClass).toContain("justify-end");
        expect(side.textClass).toContain("text-right");
        expect(side.bubbleClass).toContain("bg-emerald-50");
        expect(side.activeRingColor).toContain("emerald");
    });

    test("Microphone rows render on the left with a blue surface and the dot before the label", () => {
        const side = sourceSideLayout("Microphone");
        expect(side.isSystem).toBe(false);
        expect(side.isMic).toBe(true);
        expect(side.dotPosition).toBe("before");
        expect(side.labelRowClass).toContain("ml-1");
        expect(side.labelRowClass).not.toContain("justify-end");
        expect(side.textClass).toContain("ml-1");
        expect(side.textClass).not.toContain("text-right");
        expect(side.bubbleClass).toContain("bg-blue-50");
    });

    test("legacy and unknown devices keep a left, neutrally surfaced block", () => {
        for (const device of [undefined, "", "SystemAudio"]) {
            const side = sourceSideLayout(device);
            expect(side.isSystem).toBe(false);
            expect(side.isMic).toBe(false);
            expect(side.isLegacy).toBe(true);
            expect(side.dotPosition).toBe("before");
            expect(side.bubbleClass).toContain("bg-gray-100");
            expect(side.activeRingColor).toContain("blue");
        }
    });

    test("every run of a split block keeps the block's source side, not its cluster label", () => {
        // A System block split into SPEAKER_00 / SPEAKER_01 runs (rendered as
        // "Speaker 1" / "Speaker 2") must stay on the System side for every run.
        const runs = [
            { speaker: "SPEAKER_00", device: "System" },
            { speaker: "SPEAKER_01", device: "System" },
        ];
        const sides = runs.map(run => sourceSideLayout(run.device));
        expect(sides.every(side => side.isSystem)).toBe(true);
        expect(sides.every(side => side.dotPosition === "after")).toBe(true);
        expect(sides.every(side => side.bubbleClass.includes("bg-emerald-50"))).toBe(true);
    });
});
