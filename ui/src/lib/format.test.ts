import { afterEach, describe, expect, it } from 'vitest';
import { setLang } from './i18n';
import { briefingTitle, countsLabel, endLabel, intervalLabel, relativeTime } from './format';
import type { CountKey, EndReason } from './types';

// São Paulo has no DST since 2019: UTC-3 all year, so the expectations are stable.
const TZ = 'America/Sao_Paulo';
/** Local São Paulo wall time as epoch ms. */
const sp = (iso: string) => Date.parse(`${iso}-03:00`);
const NOW = sp('2026-10-30T14:00:00');

const card = (ended_at_ms: number | null, end_reason: EndReason | null, ended_cleanly = false) => ({
  ended_at_ms,
  end_reason,
  ended_cleanly,
});

describe('endLabel (Inv §1.5)', () => {
  afterEach(() => setLang('en'));

  it('follows the v1 table in pt-BR', () => {
    setLang('pt-BR');
    expect(endLabel(card(null, null))).toBe('em andamento');
    expect(endLabel(card(null, 'clear'))).toBe('em andamento');
    expect(endLabel(card(1, 'prompt_input_exit'))).toBe('/exit');
    expect(endLabel(card(1, 'clear'))).toBe('/clear');
    expect(endLabel(card(1, 'logout'))).toBe('logout');
    expect(endLabel(card(1, 'idle'))).toBe('sem /exit (inativa)');
    expect(endLabel(card(1, 'other', true))).toBe('encerrada');
    expect(endLabel(card(1, null, true))).toBe('encerrada');
    expect(endLabel(card(1, 'other', false))).toBe('Ctrl+C/fechada');
    expect(endLabel(card(1, null, false))).toBe('Ctrl+C/fechada');
  });

  it('has English equivalents', () => {
    setLang('en');
    expect(endLabel(card(null, null))).toBe('in progress');
    expect(endLabel(card(1, 'prompt_input_exit'))).toBe('/exit');
    expect(endLabel(card(1, 'idle'))).toBe('no /exit (idle)');
    expect(endLabel(card(1, 'other', true))).toBe('ended');
    expect(endLabel(card(1, null, false))).toBe('Ctrl+C/closed');
  });
});

describe('intervalLabel', () => {
  it('today with an end on the same day', () => {
    const start = sp('2026-10-30T10:12:00');
    const end = sp('2026-10-30T11:05:00');
    expect(intervalLabel(start, end, NOW, 'pt-BR', TZ)).toBe('hoje 10:12–11:05');
    expect(intervalLabel(start, end, NOW, 'en', TZ)).toBe('today 10:12–11:05');
  });

  it('yesterday, open or ending at the same minute', () => {
    const start = sp('2026-10-29T16:40:00');
    expect(intervalLabel(start, null, NOW, 'pt-BR', TZ)).toBe('ontem 16:40');
    expect(intervalLabel(start, start + 20_000, NOW, 'en', TZ)).toBe('yesterday 16:40');
  });

  it('another date; an end on another day is not shown', () => {
    const start = sp('2026-10-12T09:00:00');
    expect(intervalLabel(start, sp('2026-10-12T09:30:00'), NOW, 'pt-BR', TZ)).toBe('12/out 09:00–09:30');
    expect(intervalLabel(start, sp('2026-10-13T01:00:00'), NOW, 'pt-BR', TZ)).toBe('12/out 09:00');
    expect(intervalLabel(start, null, NOW, 'en', TZ)).toBe('Oct 12 09:00');
    expect(intervalLabel(sp('2026-03-05T08:00:00'), null, NOW, 'pt-BR', TZ)).toBe('05/mar 08:00');
  });

  it('uses the local day, not the UTC day', () => {
    // 22:30 local on the 29th is already the 30th in UTC.
    expect(intervalLabel(sp('2026-10-29T22:30:00'), null, NOW, 'pt-BR', TZ)).toBe('ontem 22:30');
  });
});

describe('countsLabel', () => {
  it('singular and plural, in CountKey order', () => {
    const counts: [CountKey, number][] = [
      ['stale_branch', 1],
      ['reminder', 2],
      ['ongoing', 3],
      ['interrupted', 1],
    ];
    expect(countsLabel(counts, 'pt-BR')).toBe('1 interrompida · 3 retomadas · 2 lembretes · 1 branch');
    expect(countsLabel(counts, 'en')).toBe('1 interrupted · 3 ongoings · 2 reminders · 1 branch');
  });

  it('covers every key', () => {
    const counts: [CountKey, number][] = [
      ['dirty', 2],
      ['worktree', 2],
      ['stale_branch', 2],
      ['reminder', 1],
      ['ongoing', 1],
      ['half_done', 2],
      ['interrupted', 2],
    ];
    expect(countsLabel(counts, 'pt-BR')).toBe(
      '2 interrompidas · 2 pela metade · 1 retomada · 1 lembrete · 2 branches · 2 worktrees · 2 com alterações',
    );
  });

  it('is empty without counts', () => {
    expect(countsLabel([], 'en')).toBe('');
  });
});

describe('briefingTitle', () => {
  it('drops "-feira" in pt-BR', () => {
    expect(briefingTitle('2026-10-30', 'pt-BR')).toBe('Briefing de sexta, 30/out');
    expect(briefingTitle('2026-10-26', 'pt-BR')).toBe('Briefing de segunda, 26/out');
    expect(briefingTitle('2026-11-01', 'pt-BR')).toBe('Briefing de domingo, 01/nov');
    expect(briefingTitle('2026-10-31', 'pt-BR')).toBe('Briefing de sábado, 31/out');
  });

  it('has an English form', () => {
    expect(briefingTitle('2026-10-30', 'en')).toBe('Briefing for Friday, Oct 30');
  });
});

describe('relativeTime', () => {
  it('formats past instants like the v1 diffForHumans', () => {
    const day = 86_400_000;
    expect(relativeTime(NOW - 3 * day, NOW, 'pt-BR')).toBe('há 3 dias');
    expect(relativeTime(NOW - 3 * day, NOW, 'en')).toBe('3 days ago');
    expect(relativeTime(NOW - 5 * 60_000, NOW, 'en')).toBe('5 minutes ago');
    expect(relativeTime(NOW - 2 * 3_600_000, NOW, 'pt-BR')).toBe('há 2 horas');
    expect(relativeTime(NOW - 14 * day, NOW, 'en')).toBe('2 weeks ago');
    expect(relativeTime(NOW - 65 * day, NOW, 'en')).toBe('2 months ago');
    expect(relativeTime(NOW - 400 * day, NOW, 'en')).toBe('1 year ago');
    expect(relativeTime(NOW - 10_000, NOW, 'en')).toBe('10 seconds ago');
  });
});
