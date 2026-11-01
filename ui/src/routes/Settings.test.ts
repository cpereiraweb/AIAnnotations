// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../App.svelte';
import { save_settings, set_summaries_paused, today } from '../lib/api';
import { setLang } from '../lib/i18n';
import { resetImportState } from '../lib/importState.svelte';
import { mock } from '../lib/mock';

// Delegates to the real API (and so to the mock) unless a test overrides one call.
vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>();
  return {
    ...actual,
    save_settings: vi.fn(actual.save_settings),
    set_summaries_paused: vi.fn(actual.set_summaries_paused),
    today: vi.fn(actual.today),
  };
});

beforeEach(async () => {
  setLang('pt-BR');
  resetImportState();
  const actual = await vi.importActual<typeof import('../lib/api')>('../lib/api');
  vi.mocked(save_settings).mockReset().mockImplementation(actual.save_settings);
  vi.mocked(set_summaries_paused).mockReset().mockImplementation(actual.set_summaries_paused);
  vi.mocked(today).mockReset().mockImplementation(actual.today);
  mock.reset({ delayMs: 20, onboardingDone: true });
  window.location.hash = '#/settings';
});

afterEach(() => {
  cleanup();
  setLang('en');
});

async function openSettings() {
  render(App);
  await screen.findByRole('heading', { level: 2, name: 'Processamento' });
}

const summaryModel = () => screen.getAllByLabelText('Modelo dos resumos')[0] as HTMLSelectElement;

describe('Settings', () => {
  it('settings_page_has_the_sections', async () => {
    await openSettings();
    for (const name of ['Processamento', 'Hooks', 'Histórico', 'Geral', 'Avançado']) {
      expect(screen.getByRole('heading', { level: 2, name })).toBeTruthy();
    }
    expect(screen.getByRole('navigation')).toBeTruthy();
    expect(screen.queryByText(/Passo 1 de 5/)).toBeNull();
  });

  it('changing_a_model_saves_it_at_once', async () => {
    await openSettings();
    await fireEvent.change(summaryModel(), { target: { value: 'opus' } });
    await waitFor(() => expect(save_settings).toHaveBeenCalledWith({ claude_model: 'opus' }));
    expect((await screen.findByRole('status')).textContent).toContain('Ajustes salvos.');
  });

  it('last_change_wins', async () => {
    await openSettings();
    await fireEvent.change(summaryModel(), { target: { value: 'sonnet' } });
    await fireEvent.change(summaryModel(), { target: { value: 'opus' } });
    await waitFor(() => expect(save_settings).toHaveBeenCalledTimes(2));
    expect(vi.mocked(save_settings).mock.calls.at(-1)![0]).toEqual({ claude_model: 'opus' });
    await new Promise((r) => setTimeout(r, 80));
    expect(summaryModel().value).toBe('opus');
  });

  it('save_error_restores_the_previous_value', async () => {
    await openSettings();
    expect(summaryModel().value).toBe('haiku');
    vi.mocked(save_settings).mockRejectedValueOnce('boom');
    await fireEvent.change(summaryModel(), { target: { value: 'opus' } });
    await waitFor(() => expect(summaryModel().value).toBe('haiku'));
    expect((await screen.findByRole('status')).textContent).toContain('boom');
  });

  it('language_switch_changes_the_ui', async () => {
    await openSettings();
    await fireEvent.change(screen.getByLabelText('Idioma'), { target: { value: 'en' } });
    await waitFor(() => expect(save_settings).toHaveBeenCalledWith({ lang: 'en' }));
    expect(await screen.findByRole('heading', { level: 2, name: 'Processing' })).toBeTruthy();
  });

  it('automatic_language_saves_null', async () => {
    await openSettings();
    await fireEvent.change(screen.getByLabelText('Idioma'), { target: { value: 'en' } });
    await screen.findByRole('heading', { level: 2, name: 'Processing' });
    await fireEvent.change(screen.getByLabelText('Language'), { target: { value: '' } });
    await waitFor(() => expect(save_settings).toHaveBeenLastCalledWith({ lang: null }));
  });

  it('autostart_toggle_saves', async () => {
    await openSettings();
    await fireEvent.click(screen.getByLabelText('Iniciar com o sistema'));
    await waitFor(() => expect(save_settings).toHaveBeenCalledWith({ autostart: expect.any(Boolean) }));
  });

  it('pause_toggle_calls_the_backend', async () => {
    await openSettings();
    await fireEvent.click(screen.getByLabelText('Pausar resumos e briefing'));
    await waitFor(() => expect(set_summaries_paused).toHaveBeenCalledWith(true));
  });

  it('history_progress_line_shows_pending_backfill', async () => {
    await openSettings();
    expect(await screen.findByText('histórico: 42/120 resumidas')).toBeTruthy();
  });

  it('redo_first_run_opens_the_wizard', async () => {
    await openSettings();
    await fireEvent.click(screen.getByRole('button', { name: 'Refazer a primeira execução' }));
    expect(window.location.hash).toBe('#/onboarding');
    expect(await screen.findByRole('heading', { level: 1, name: 'Primeira abertura' })).toBeTruthy();
    expect(screen.getByText('Passo 1 de 5')).toBeTruthy();
  });

  it('nav_link_marks_settings_as_current', async () => {
    await openSettings();
    const nav = screen.getByRole('navigation');
    expect(within(nav).getByRole('link', { name: /Ajustes/ }).getAttribute('aria-current')).toBe('page');
  });
});
