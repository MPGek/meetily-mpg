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

/**
 * Palette key -> Tailwind pill classes (light theme, vetted contrast).
 * 40 distinct keys: 22 bare hues plus 18 deeper-shade variants. Keys must
 * match the backend `MEETING_TAG_PALETTE` (change: tags-persistence-and-palette);
 * `tag-palette.json` is the parity reference used by the tests.
 * All classes are literal so Tailwind keeps them in the build.
 */
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
  red: 'bg-red-100 text-red-800 border-red-200',
  yellow: 'bg-yellow-100 text-yellow-800 border-yellow-200',
  emerald: 'bg-emerald-100 text-emerald-800 border-emerald-200',
  sky: 'bg-sky-100 text-sky-800 border-sky-200',
  indigo: 'bg-indigo-100 text-indigo-800 border-indigo-200',
  violet: 'bg-violet-100 text-violet-800 border-violet-200',
  pink: 'bg-pink-100 text-pink-800 border-pink-200',
  slate: 'bg-slate-100 text-slate-800 border-slate-200',
  gray: 'bg-gray-100 text-gray-800 border-gray-200',
  zinc: 'bg-zinc-100 text-zinc-800 border-zinc-200',
  neutral: 'bg-neutral-100 text-neutral-800 border-neutral-200',
  stone: 'bg-stone-100 text-stone-800 border-stone-200',
  'blue-deep': 'bg-blue-200 text-blue-900 border-blue-300',
  'green-deep': 'bg-green-200 text-green-900 border-green-300',
  'purple-deep': 'bg-purple-200 text-purple-900 border-purple-300',
  'amber-deep': 'bg-amber-200 text-amber-900 border-amber-300',
  'rose-deep': 'bg-rose-200 text-rose-900 border-rose-300',
  'cyan-deep': 'bg-cyan-200 text-cyan-900 border-cyan-300',
  'teal-deep': 'bg-teal-200 text-teal-900 border-teal-300',
  'orange-deep': 'bg-orange-200 text-orange-900 border-orange-300',
  'lime-deep': 'bg-lime-200 text-lime-900 border-lime-300',
  'fuchsia-deep': 'bg-fuchsia-200 text-fuchsia-900 border-fuchsia-300',
  'red-deep': 'bg-red-200 text-red-900 border-red-300',
  'yellow-deep': 'bg-yellow-200 text-yellow-900 border-yellow-300',
  'emerald-deep': 'bg-emerald-200 text-emerald-900 border-emerald-300',
  'sky-deep': 'bg-sky-200 text-sky-900 border-sky-300',
  'indigo-deep': 'bg-indigo-200 text-indigo-900 border-indigo-300',
  'violet-deep': 'bg-violet-200 text-violet-900 border-violet-300',
  'pink-deep': 'bg-pink-200 text-pink-900 border-pink-300',
  'slate-deep': 'bg-slate-200 text-slate-900 border-slate-300',
};

export const TAG_PALETTE_KEYS = Object.keys(TAG_PILL_STYLES);

export function tagPillClass(color: string): string {
  return TAG_PILL_STYLES[color] ?? TAG_PILL_STYLES.blue;
}

/** Next palette key after `color`, wrapping from the last entry to the first. */
export function nextColor(color: string): string {
  const i = TAG_PALETTE_KEYS.indexOf(color);
  return TAG_PALETTE_KEYS[(i + 1 + TAG_PALETTE_KEYS.length) % TAG_PALETTE_KEYS.length];
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
