/**
 * Source-side layout rule for transcript rows (split-transcript-ui), shared by
 * the single-record variants and the live word-level sub-rows: System content
 * renders on the right with the dot after the label, Microphone and legacy
 * content on the left with the dot before it. The side follows `source_device`
 * only, never a row's cluster label, so a split block's runs and the block's
 * own surface stay on the channel they came from.
 */
export interface SourceSideLayout {
    /** True for the System channel (right side, emerald surface). */
    isSystem: boolean;
    /** True for the Microphone channel. */
    isMic: boolean;
    /** True when the record carries no channel (legacy rows). */
    isLegacy: boolean;
    /** Where the speaker dot sits relative to the label. */
    dotPosition: 'before' | 'after';
    /** Classes for a label line (dot + speaker label). */
    labelRowClass: string;
    /** Classes for the row's text. */
    textClass: string;
    /** Background + border classes of the row's block surface. */
    bubbleClass: string;
    /** Ring color used when the row is the active playback target. */
    activeRingColor: string;
}

export function sourceSideLayout(sourceDevice?: string): SourceSideLayout {
    const isSystem = sourceDevice === 'System';
    const isMic = sourceDevice === 'Microphone';
    return {
        isSystem,
        isMic,
        isLegacy: !isSystem && !isMic,
        dotPosition: isSystem ? 'after' : 'before',
        labelRowClass: `flex items-center gap-1.5 mb-1 ${isSystem ? 'justify-end mr-1' : 'ml-1'}`,
        textClass: `text-sm text-gray-800 leading-relaxed whitespace-pre-wrap ${isSystem ? 'text-right mr-1' : 'ml-1'}`,
        bubbleClass: isSystem
            ? 'bg-emerald-50 border border-emerald-100'
            : isMic
                ? 'bg-blue-50 border border-blue-100'
                : 'bg-gray-100 border border-gray-200',
        activeRingColor: isSystem ? 'ring-emerald-400' : 'ring-blue-400',
    };
}
