export interface MeetingTag {
  id: string;
  name: string;
  color: string;
  created_at?: string;
  updated_at?: string;
}

export interface MeetingTagWithUsage extends MeetingTag {
  usage_count: number;
}

/** Palette key -> Tailwind pill classes (light theme, vetted contrast). */
export const TAG_PILL_STYLES: Record<string, string> = {
  blue: 'bg-blue-100 text-blue-800 border-blue-200',
  green: 'bg-green-100 text-green-800 border-green-200',
  purple: 'bg-purple-100 text-purple-800 border-purple-200',
  amber: 'bg-amber-100 text-amber-800 border-amber-200',
  rose: 'bg-rose-100 text-rose-800 border-rose-200',
  cyan: 'bg-cyan-100 text-cyan-800 border-cyan-200',
  teal: 'bg-teal-100 text-teal-800 border-teal-200',
  orange: 'bg-orange-100 text-orange-800 border-orange-200',
  lime: 'bg-lime-100 text-lime-800 border-lime-200',
  fuchsia: 'bg-fuchsia-100 text-fuchsia-800 border-fuchsia-200',
};

export const TAG_PALETTE_KEYS = Object.keys(TAG_PILL_STYLES);

export function tagPillClass(color: string): string {
  return TAG_PILL_STYLES[color] ?? TAG_PILL_STYLES.blue;
}

/**
 * Format an RFC3339/ISO timestamp as `yyyy-mm-dd hh:mm` in the user's local
 * timezone (24h). Returns `--` for missing/unparsable input so the list row
 * never breaks.
 */
export function formatMeetingDate(createdAt: string | null | undefined): string {
  if (!createdAt) return '--';
  const d = new Date(createdAt);
  if (Number.isNaN(d.getTime())) return '--';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/**
 * Heuristic `lang` for a meeting title so CSS `hyphens: auto` picks the right
 * hyphenation dictionary (Chromium/WebKit bundle ru+en and others).
 * Returns undefined when nothing matches — the element then inherits the
 * document language.
 */
export function detectTitleLang(title: string): string | undefined {
  if (/[а-яё]/i.test(title)) return 'ru';
  if (/[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff]/.test(title)) return 'zh';
  if (/[a-z]/i.test(title)) return 'en';
  return undefined;
}
