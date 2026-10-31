// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../App.svelte';
import { finish_onboarding, onboarding_state, write_hooks } from '../lib/api';
import { setLang } from '../lib/i18n';
import { mock } from '../lib/mock';
import type { OnboardingView } from '../lib/types';

// Delegates to the real API (and so to the mock) unless a test overrides one call.
vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>();
  return {
    ...actual,
    onboarding_state: vi.fn(actual.onboarding_state),
    finish_onboarding: vi.fn(actual.finish_onboarding),
    write_hooks: vi.fn(actual.write_hooks),
  };
});

beforeEach(() => {
  setLang('pt-BR');
  vi.mocked(write_hooks).mockClear();
  vi.mocked(finish_onboarding).mockClear();
  mock.reset({ delayMs: 20, onboardingDone: false });
  window.location.hash = '#/';
});

afterEach(() => {
  cleanup();
  setLang('en');
});

async function openWizard() {
  render(App);
  await screen.findByRole('heading', { name: 'Detecção' });
}

async function next(times = 1) {
  for (let i = 0; i < times; i++) await fireEvent.click(screen.getByRole('button', { name: 'Continuar' }));
}

async function withView(over: (v: OnboardingView) => void) {
  const actual = await vi.importActual<typeof import('../lib/api')>('../lib/api');
  vi.mocked(onboarding_state).mockImplementation(async () => {
    const v = await actual.onboarding_state();
    over(v);
    return v;
  });
}

describe('Onboarding', () => {
  afterEach(async () => {
    const actual = await vi.importActual<typeof import('../lib/api')>('../lib/api');
    vi.mocked(onboarding_state).mockImplementation(actual.onboarding_state);
  });

  it('shows_the_wizard_without_the_layout_until_done', async () => {
    await openWizard();
    expect(screen.queryByRole('navigation')).toBeNull();
  });

  it('shows_not_found_without_blocking', async () => {
    await openWizard();
    expect(screen.getByText(/Claude Code.*encontrado em \/usr\/bin\/claude/)).toBeTruthy();
    expect(screen.getByText(/Codex.*não encontrado/)).toBeTruthy();
    // Nothing blocks the next step.
    const button = screen.getByRole('button', { name: 'Continuar' }) as HTMLButtonElement;
    expect(button.disabled).toBe(false);
    await next();
    expect(screen.getByRole('heading', { name: 'Hooks' })).toBeTruthy();
  });

  it('write_hook_button_disabled_for_invalid_json', async () => {
    await withView((v) => {
      v.cards[0].invalid_json = true;
      v.cards[0].hook_status = null;
      v.cards[0].diff = null;
      v.cards[0].can_write = false;
    });
    await openWizard();
    await next();
    expect((screen.getByRole('button', { name: 'Gravar hook' }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText('Arquivo inválido; nada foi alterado')).toBeTruthy();
  });

  it('write_hook_button_disabled_without_diff', async () => {
    await withView((v) => {
      v.cards[0].can_write = true;
      v.cards[0].diff = null;
      v.cards[0].hook_status = 'not_installed';
    });
    await openWizard();
    await next();
    const button = screen.getByRole('button', { name: 'Gravar hook' }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    await fireEvent.click(button);
    expect(write_hooks).not.toHaveBeenCalled();
  });

  it('write_hook_shows_the_diff_and_marks_the_hook_installed', async () => {
    await openWizard();
    await next();
    expect(document.querySelector('pre')?.textContent).toContain('SessionStart');
    await fireEvent.click(screen.getByRole('button', { name: 'Gravar hook' }));
    expect(await screen.findByText('Hook já instalado')).toBeTruthy();
  });

  it('warns_when_the_app_cli_is_missing', async () => {
    await withView((v) => {
      v.cli_found = false;
      v.cards[0].can_write = false;
      v.cards[0].diff = null;
    });
    await openWizard();
    await next();
    expect(screen.getByText('O executável aiannotations não foi encontrado ao lado do app')).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Gravar hook' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('codex_trust_notice_after_writing', async () => {
    mock.reset({ delayMs: 20, onboardingDone: false, codexFound: true });
    await openWizard();
    await next();
    expect(screen.queryByText(/Abra \/hooks no Codex/)).toBeNull();
    const buttons = screen.getAllByRole('button', { name: 'Gravar hook' });
    await fireEvent.click(buttons[1]);
    expect(
      await screen.findByText('Abra /hooks no Codex e confie nos hooks do AIAnnotations; sem isso eles não rodam.'),
    ).toBeTruthy();
  });

  it('processing_warns_when_no_cli_is_detected', async () => {
    await withView((v) => {
      v.cards[0].bin = null;
    });
    await openWizard();
    await next(2);
    expect(screen.getByText(/os resumos ficam desligados até você instalar um/)).toBeTruthy();
  });

  it('processing_prefills_the_models_of_the_provider', async () => {
    await openWizard();
    await next(2);
    expect((screen.getByLabelText('Modelo dos resumos') as HTMLInputElement).value).toBe('haiku');
    expect((screen.getByLabelText('Modelo do briefing') as HTMLInputElement).value).toBe('sonnet');
  });

  it('import_shows_progress_then_the_counts', async () => {
    await openWizard();
    await next(3);
    await fireEvent.click(screen.getByRole('button', { name: 'Importar' }));
    expect(await screen.findByRole('progressbar')).toBeTruthy();
    expect(await screen.findByText(/sessões em 3 projetos — \d+ Claude Code, \d+ Codex/)).toBeTruthy();
  });

  it('history_ai_opt_in_is_unchecked_and_shows_estimate', async () => {
    await openWizard();
    await next(3);
    const box = screen.getByRole('checkbox', { name: 'Resumir o histórico com IA' }) as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect(screen.getByText('Usa a cota do Claude Code: cerca de 78 execuções, uma a cada 2 minutos.')).toBeTruthy();
  });

  it('finish_sends_choices', async () => {
    await openWizard();
    await next(4);
    await fireEvent.click(screen.getByRole('button', { name: 'Concluir' }));
    await waitFor(() => expect(finish_onboarding).toHaveBeenCalled());
    expect(vi.mocked(finish_onboarding).mock.calls[0][0]).toEqual({
      provider: 'claude',
      claude_model: 'haiku',
      claude_briefing_model: 'sonnet',
      codex_model: '',
      codex_briefing_model: '',
      summarize_history: false,
      autostart: true,
    });
  });

  it('finish_goes_to_today_with_the_layout', async () => {
    await openWizard();
    await next(4);
    await fireEvent.click(screen.getByRole('button', { name: 'Concluir' }));
    expect(await screen.findByRole('navigation')).toBeTruthy();
    expect(window.location.hash).toBe('#/');
  });

  it('finish_toasts_the_autostart_warning_but_still_goes_to_today', async () => {
    vi.mocked(finish_onboarding).mockResolvedValueOnce('permission denied');
    await openWizard();
    await next(4);
    await fireEvent.click(screen.getByRole('button', { name: 'Concluir' }));
    expect(await screen.findByRole('navigation')).toBeTruthy();
    expect((await screen.findByRole('status')).textContent).toContain('permission denied');
  });

  it('back_returns_to_the_previous_step', async () => {
    await openWizard();
    await next();
    await fireEvent.click(screen.getByRole('button', { name: 'Voltar' }));
    expect(screen.getByRole('heading', { name: 'Detecção' })).toBeTruthy();
  });
});
