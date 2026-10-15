import type { Block, Summary } from '@/types';

/**
 * Legacy-format summary check used once generation completes: true when every
 * section has no `blocks` or an empty `blocks` array (lifted from
 * `useSummaryGeneration`).
 */
export function isLegacySummaryEmpty(sections: [string, unknown][]): boolean {
  return sections.every(([, section]) => {
    const typedSection = section as { blocks?: unknown[] };
    return !typedSection.blocks || typedSection.blocks.length === 0;
  });
}

/**
 * Formats legacy section data into a `Summary`, visiting keys in `sectionOrder`.
 * Missing titles default to the section key, block content is trimmed and every
 * block gets `color: 'default'`; non-section values are skipped.
 */
export function formatLegacySummaryData(
  sectionOrder: string[],
  summaryData: Record<string, unknown>
): Summary {
  const formattedSummary: Summary = {};

  for (const key of sectionOrder) {
    try {
      const section = summaryData[key];
      if (section && typeof section === 'object' && 'title' in section && 'blocks' in section) {
        const typedSection = section as { title?: string; blocks?: Block[] };

        if (Array.isArray(typedSection.blocks)) {
          formattedSummary[key] = {
            title: typedSection.title || key,
            blocks: typedSection.blocks.map((block) => ({
              ...block,
              color: 'default',
              content: block?.content?.trim() || ''
            }))
          };
        } else {
          formattedSummary[key] = {
            title: typedSection.title || key,
            blocks: []
          };
        }
      }
    } catch (error) {
      console.warn(`Error processing section ${key}:`, error);
    }
  }

  return formattedSummary;
}
