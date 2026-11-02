// Formatting of the backend's raw data (ms UTC, enums, numbers) in the system time zone and the
// UI language. `timeZone` parameters default to the system zone; tests pass one explicitly.
import { t, tr } from './i18n';
import { COUNT_KEYS, type CountKey, type Lang, type RunCard } from './types';

const MONTHS: Record<Lang, string[]> = {
  'pt-BR': ['jan', 'fev', 'mar', 'abr', 'mai', 'jun', 'jul', 'ago', 'set', 'out', 'nov', 'dez'],
  en: ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'],
};

/** Without "-feira", as the v1 title. */
const WEEKDAYS: Record<Lang, string[]> = {
  'pt-BR': ['domingo', 'segunda', 'terça', 'quarta', 'quinta', 'sexta', 'sábado'],
  en: ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'],
};

interface LocalParts {
  year: number;
  month: number; // 1–12
  day: number;
  time: string; // HH:mm
}

const partsFormatters = new Map<string, Intl.DateTimeFormat>();

function localParts(ms: number, timeZone?: string): LocalParts {
  const cacheKey = timeZone ?? '';
  let fmt = partsFormatters.get(cacheKey);
  if (!fmt) {
    fmt = new Intl.DateTimeFormat('en-US', {
      timeZone,
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      hourCycle: 'h23',
    });
    partsFormatters.set(cacheKey, fmt);
  }
  const p: Record<string, string> = {};
  for (const part of fmt.formatToParts(new Date(ms))) p[part.type] = part.value;
  return {
    year: Number(p.year),
    month: Number(p.month),
    day: Number(p.day),
    time: `${p.hour}:${p.minute}`,
  };
}

/** Days between two local calendar dates (b − a). */
function dayDiff(a: LocalParts, b: LocalParts): number {
  return Math.round((Date.UTC(b.year, b.month - 1, b.day) - Date.UTC(a.year, a.month - 1, a.day)) / 86_400_000);
}

const pad = (n: number) => String(n).padStart(2, '0');

/** "12/out" or "Oct 12". */
function shortDate(day: number, month: number, lang: Lang): string {
  const name = MONTHS[lang][month - 1];
  return lang === 'pt-BR' ? `${pad(day)}/${name}` : `${name} ${day}`;
}

/** "há 3 dias" / "3 days ago", with the largest whole unit (like Carbon's diffForHumans). */
export function relativeTime(ms: number, now: number, lang: Lang): string {
  const rtf = new Intl.RelativeTimeFormat(lang, { numeric: 'always' });
  const diff = ms - now;
  const abs = Math.abs(diff);
  const sign = diff < 0 ? -1 : 1;
  const units: [Intl.RelativeTimeFormatUnit, number][] = [
    ['year', 365 * 86_400_000],
    ['month', 30 * 86_400_000],
    ['week', 7 * 86_400_000],
    ['day', 86_400_000],
    ['hour', 3_600_000],
    ['minute', 60_000],
  ];
  for (const [unit, size] of units) {
    if (abs >= size) return rtf.format(sign * Math.floor(abs / size), unit);
  }
  return rtf.format(sign * Math.floor(abs / 1000), 'second');
}

/**
 * "hoje 10:12–11:05", "ontem 16:40", "12/out 09:00" (SessionRun::intervalLabel). The end time is
 * shown only when it is on the same local day and at a different minute.
 */
export function intervalLabel(
  start: number,
  end: number | null,
  now: number,
  lang: Lang,
  timeZone?: string,
): string {
  const s = localParts(start, timeZone);
  const days = dayDiff(s, localParts(now, timeZone));
  const day =
    days === 0
      ? tr(lang, 'interval.today')
      : days === 1
        ? tr(lang, 'interval.yesterday')
        : shortDate(s.day, s.month, lang);
  let label = `${day} ${s.time}`;
  if (end !== null) {
    const e = localParts(end, timeZone);
    if (dayDiff(s, e) === 0 && e.time !== s.time) label += `–${e.time}`;
  }
  return label;
}

/** How the run ended (SessionRun::endLabel, Inv §1.5), in the current language. */
export function endLabel(card: Pick<RunCard, 'ended_at_ms' | 'end_reason' | 'ended_cleanly'>): string {
  if (card.ended_at_ms === null) return t('end.in_progress');
  switch (card.end_reason) {
    case 'prompt_input_exit':
      return t('end.exit');
    case 'clear':
      return t('end.clear');
    case 'logout':
      return t('end.logout');
    case 'idle':
      return t('end.idle');
    default:
      return card.ended_cleanly ? t('end.clean') : t('end.killed');
  }
}

/** "1 interrompida · 3 retomadas · 2 lembretes · 1 branch", always in CountKey order. */
export function countsLabel(counts: [CountKey, number][], lang: Lang): string {
  return [...counts]
    .sort(([a], [b]) => COUNT_KEYS.indexOf(a) - COUNT_KEYS.indexOf(b))
    .map(([key, count]) => tr(lang, `counts.${key}`, { count }))
    .join(' · ');
}

/** "Briefing de sexta, 30/out" / "Briefing for Friday, Oct 30", from the `YYYY-MM-DD` date. */
export function briefingTitle(date: string, lang: Lang): string {
  const [y, m, d] = date.split('-').map(Number);
  const weekday = WEEKDAYS[lang][new Date(Date.UTC(y, m - 1, d)).getUTCDay()];
  return tr(lang, 'briefing.title', { date: `${weekday}, ${shortDate(d, m, lang)}` });
}
