// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { setLang } from '../lib/i18n';
import type { Briefing } from '../lib/types';
import BriefingCard from './BriefingCard.svelte';

beforeEach(() => setLang('pt-BR'));
afterEach(() => {
  cleanup();
  setLang('en');
});

const generatingRow: Briefing = {
  id: 1,
  date: '2026-10-30',
  status: 'generating',
  body: null,
  next_steps: [],
  covers_from_ms: 0,
  covers_until_ms: 0,
  generated_at_ms: null,
};

function show(props: { paused?: boolean; startFailed?: boolean; generating?: boolean }) {
  render(BriefingCard, {
    briefing: generatingRow,
    generating: props.generating ?? false,
    paused: props.paused ?? false,
    startFailed: props.startFailed ?? false,
    today: '2026-10-30',
    onregenerate: () => {},
  });
}

describe('BriefingCard', () => {
  it('says the summaries are paused instead of "gerando…" when nothing generates', () => {
    show({ paused: true });
    expect(screen.getByText(/Resumos pausados/)).toBeTruthy();
    expect(screen.queryByText('gerando…')).toBeNull();
    expect(screen.getByText('regerar')).toBeTruthy();
  });

  it('shows the failure when the automatic start failed', () => {
    show({ startFailed: true });
    expect(screen.getByText('Não foi possível gerar o briefing agora.')).toBeTruthy();
    expect(screen.queryByText('gerando…')).toBeNull();
  });

  it('a running generation wins over the pause', () => {
    show({ paused: true, generating: true });
    expect(screen.getByText('gerando…')).toBeTruthy();
  });
});
