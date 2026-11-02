// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../App.svelte';
import { apps, save_settings, set_summaries_paused, today } from '../lib/api';
import { setLang } from '../lib/i18n';
import { resetAppsState } from '../lib/apps.svelte';
import { toastState } from '../lib/toast.svelte';
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
    apps: vi.fn(actual.apps),
  };
});

beforeEach(async () => {
  setLang('pt-BR');
  resetImportState();
  resetAppsState();
  toastState.message = '';
  const actual = await vi.importActual<typeof import('../lib/api')>('../lib/api');
  vi.mocked(save_settings).mockReset().mockImplementation(actual.save_settings);
  vi.mocked(set_summaries_paused).mockReset().mockImplementation(actual.set_summaries_paused);
  vi.mocked(today).mockReset().mockImplementation(actual.today);
  vi.mocked(apps).mockReset().mockImplementation(actual.apps);
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
    for (const name of ['Processamento', 'Hooks', 'Histórico', 'Editores e terminais', 'Geral', 'Avançado']) {
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

  it('pause_box_starts_from_the_apps_own_flag', async () => {
    // The app is paused (tray), the worker's status has not reported it yet (`false` in the mock).
    const actual = await vi.importActual<typeof import('../lib/api')>('../lib/api');
    vi.mocked(today).mockImplementation(async () => ({ ...(await actual.today()), summaries_paused: true }));
    await openSettings();
    await waitFor(() =>
      expect((screen.getByLabelText('Pausar resumos e briefing') as HTMLInputElement).checked).toBe(true),
    );
  });

  it('pause_box_follows_a_pause_made_elsewhere', async () => {
    await openSettings();
    const box = screen.getByLabelText('Pausar resumos e briefing') as HTMLInputElement;
    await waitFor(() => expect(today).toHaveBeenCalled());
    expect(box.checked).toBe(false);
    // The tray pauses: the backend only emits "changed".
    const actual = await vi.importActual<typeof import('../lib/api')>('../lib/api');
    await actual.set_summaries_paused(true);
    await waitFor(() => expect(box.checked).toBe(true));
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

  describe('editors and terminals', () => {
    const openApps = async () => {
      await openSettings();
      await screen.findByLabelText('Editor padrão');
    };
    const select = (name: string) => screen.getByLabelText(name) as HTMLSelectElement;

    it('settings_shows_effective_defaults', async () => {
      await openApps();
      const editor = select('Editor padrão');
      expect(editor.selectedOptions[0].textContent).toBe('Automático (VS Code)');
      expect(select('Terminal padrão').selectedOptions[0].textContent).toBe('Automático (Kitty)');
      expect(select('Shell').selectedOptions[0].textContent).toBe('Automático (/bin/bash)');
      expect(editor.value).toBe('');
    });

    it('settings_saves_editor_choice', async () => {
      await openApps();
      await fireEvent.change(select('Editor padrão'), { target: { value: 'cursor' } });
      await waitFor(() => expect(save_settings).toHaveBeenCalledWith({ editor: 'cursor' }));
      await waitFor(() => expect(select('Editor padrão').value).toBe('cursor'));
    });

    it('settings_auto_sends_null', async () => {
      await openApps();
      await fireEvent.change(select('Editor padrão'), { target: { value: 'cursor' } });
      await waitFor(() => expect(select('Editor padrão').value).toBe('cursor'));
      await fireEvent.change(select('Editor padrão'), { target: { value: '' } });
      await waitFor(() => expect(save_settings).toHaveBeenLastCalledWith({ editor: null }));
    });

    it('settings_saves_workspaces_dir_and_empty_is_null', async () => {
      await openApps();
      const field = screen.getByLabelText('Pasta de workspaces (.code-workspace)');
      await fireEvent.change(field, { target: { value: '/home/dev/ws' } });
      await waitFor(() => expect(save_settings).toHaveBeenCalledWith({ workspaces_dir: '/home/dev/ws' }));
      await fireEvent.change(field, { target: { value: '  ' } });
      await waitFor(() => expect(save_settings).toHaveBeenLastCalledWith({ workspaces_dir: null }));
    });

    async function fillCustom(args = '{dir}') {
      await fireEvent.change(screen.getByLabelText('Tipo'), { target: { value: 'editor' } });
      await fireEvent.input(screen.getByLabelText('Nome'), { target: { value: 'Zed' } });
      await fireEvent.input(screen.getByLabelText('Executável'), { target: { value: '/usr/bin/zed' } });
      await fireEvent.input(screen.getByLabelText('Argumentos'), { target: { value: args } });
      await fireEvent.click(screen.getByRole('button', { name: 'Adicionar' }));
    }

    it('settings_adds_custom_app', async () => {
      await openApps();
      expect(screen.getByText('Use {dir} para a pasta e {cmd} para o comando.')).toBeTruthy();
      await fillCustom();
      await waitFor(() =>
        expect(save_settings).toHaveBeenCalledWith({
          custom_apps: [{ id: '', label: 'Zed', kind: 'editor', exe: '/usr/bin/zed', args: '{dir}' }],
        }),
      );
      expect(await screen.findByRole('button', { name: 'Remover Zed' })).toBeTruthy();
      await waitFor(() => expect(select('Editor padrão').textContent).toContain('Zed'));
    });

    it('settings_removes_custom_app', async () => {
      await openApps();
      await fillCustom();
      await fireEvent.click(await screen.findByRole('button', { name: 'Remover Zed' }));
      await waitFor(() => expect(save_settings).toHaveBeenLastCalledWith({ custom_apps: [] }));
      await waitFor(() => expect(screen.queryByRole('button', { name: 'Remover Zed' })).toBeNull());
    });

    it('settings_custom_app_error_rolls_back', async () => {
      await openApps();
      vi.mocked(save_settings).mockRejectedValueOnce('invalid template');
      await fillCustom('sem-marcador');
      await waitFor(() => expect(screen.getByRole('status').textContent).toContain('O modelo de argumentos é inválido'));
      await waitFor(() => expect(screen.queryByRole('button', { name: 'Remover Zed' })).toBeNull());
      expect(select('Editor padrão').textContent).not.toContain('Zed');
    });

    it('settings_custom_app_keeps_fields_after_rejected_add', async () => {
      await openApps();
      vi.mocked(save_settings).mockRejectedValueOnce('executable not found');
      await fillCustom('--new {dir}');
      await waitFor(() => expect(screen.getByRole('status').textContent).toContain('O executável não foi encontrado'));
      const value = (name: string) => (screen.getByLabelText(name) as HTMLInputElement).value;
      expect(value('Nome')).toBe('Zed');
      expect(value('Executável')).toBe('/usr/bin/zed');
      expect(value('Argumentos')).toBe('--new {dir}');
      expect(select('Tipo').value).toBe('editor');

      // Fixed and sent again, it is saved and the form clears.
      await fireEvent.click(screen.getByRole('button', { name: 'Adicionar' }));
      expect(await screen.findByRole('button', { name: 'Remover Zed' })).toBeTruthy();
      await waitFor(() => expect(value('Nome')).toBe(''));
      expect(value('Executável')).toBe('');
      expect(value('Argumentos')).toBe('');
    });

    it('settings_redetect_calls_refresh', async () => {
      await openApps();
      await fireEvent.click(screen.getByRole('button', { name: 'Detectar de novo' }));
      await waitFor(() => expect(apps).toHaveBeenCalledWith(true));
    });

    it('settings_translates_launch_error_codes', async () => {
      await openApps();
      vi.mocked(save_settings).mockRejectedValueOnce('executable not found');
      await fireEvent.change(select('Editor padrão'), { target: { value: 'cursor' } });
      await waitFor(() => expect(screen.getByRole('status').textContent).toContain('O executável não foi encontrado'));
    });
  });
});
