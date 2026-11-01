// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../App.svelte';
import { NO_BACKEND, finish_onboarding, onImportDone, onboarding_state, remove_hooks, write_hooks } from '../lib/api';
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
    remove_hooks: vi.fn(actual.remove_hooks),
    onImportDone: vi.fn(actual.onImportDone),
  };
});

beforeEach(() => {
  setLang('pt-BR');
  vi.mocked(write_hooks).mockClear();
  vi.mocked(remove_hooks).mockClear();
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

  it('laravel_hooks_are_kept_unless_the_user_ticks_replace', async () => {
    await withView((v) => {
      v.cards[0].legacy = true;
      v.cards[0].diff = 'DIFF AO LADO';
      v.cards[0].diff_replace = 'DIFF SUBSTITUINDO';
    });
    await openWizard();
    await next();
    const box = screen.getByRole('checkbox', { name: 'Substituir os hooks do app Laravel' }) as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect(document.querySelector('pre')?.textContent).toBe('DIFF AO LADO');
    await fireEvent.click(box);
    expect(document.querySelector('pre')?.textContent).toBe('DIFF SUBSTITUINDO');
    await fireEvent.click(screen.getByRole('button', { name: 'Gravar hook' }));
    expect(write_hooks).toHaveBeenCalledWith('claude', true);
  });

  it('writes_beside_the_laravel_hooks_by_default', async () => {
    await withView((v) => {
      v.cards[0].legacy = true;
      v.cards[0].diff_replace = 'DIFF SUBSTITUINDO';
    });
    await openWizard();
    await next();
    await fireEvent.click(screen.getByRole('button', { name: 'Gravar hook' }));
    expect(write_hooks).toHaveBeenCalledWith('claude', false);
  });

  it('current_hook_beside_laravel_can_still_replace_them', async () => {
    await withView((v) => {
      v.cards[0].hook_status = 'current';
      v.cards[0].legacy = true;
      v.cards[0].diff = null;
      v.cards[0].diff_replace = 'DIFF SUBSTITUINDO';
    });
    await openWizard();
    await next();
    const button = screen.getByRole('button', { name: 'Gravar hook' }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    expect(screen.getByText('Hook já instalado')).toBeTruthy();
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Substituir os hooks do app Laravel' }));
    expect(button.disabled).toBe(false);
    expect(screen.queryByText('Hook já instalado')).toBeNull();
  });

  it('no_replace_option_without_laravel_hooks', async () => {
    await openWizard();
    await next();
    expect(screen.queryByRole('checkbox', { name: 'Substituir os hooks do app Laravel' })).toBeNull();
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

  const block = (name: string) => within(screen.getByRole('group', { name }));
  const select = (tool: string, label: string) => block(tool).getByLabelText(label) as HTMLSelectElement;

  it('processing_prefills_the_models_of_the_provider', async () => {
    await openWizard();
    await next(2);
    const summary = select('Claude Code', 'Modelo dos resumos');
    expect(summary.tagName).toBe('SELECT');
    expect(summary.value).toBe('haiku');
    expect([...summary.options].map((o) => o.value)).toEqual(['haiku', 'sonnet', 'opus', 'fable']);
    expect(select('Claude Code', 'Modelo do briefing').value).toBe('sonnet');
  });

  it('processing_shows_both_clis_and_disables_the_one_not_found', async () => {
    await openWizard();
    await next(2);
    expect((screen.getByRole('radio', { name: 'Claude Code' }) as HTMLInputElement).checked).toBe(true);
    const codex = screen.getByRole('radio', { name: 'Codex' }) as HTMLInputElement;
    expect(codex.disabled).toBe(true);
    expect((screen.getByRole('group', { name: 'Codex' }) as HTMLFieldSetElement).disabled).toBe(true);
    expect(screen.getByText('não encontrado')).toBeTruthy();
    expect(screen.getByText('em uso')).toBeTruthy();
  });

  it('codex_models_select_starts_on_the_codex_default', async () => {
    mock.reset({ delayMs: 20, onboardingDone: false, codexFound: true });
    await openWizard();
    await next(2);
    // Both blocks are editable whatever processes; the segmented control picks the provider.
    await fireEvent.change(select('Claude Code', 'Modelo dos resumos'), { target: { value: 'opus' } });
    await fireEvent.click(screen.getByRole('radio', { name: 'Codex' }));
    const summary = select('Codex', 'Modelo dos resumos');
    expect(summary.value).toBe('');
    expect([...summary.options].map((o) => o.text)).toEqual(['Padrão do Codex', 'GPT Mock', 'GPT Mock Mini']);
    await fireEvent.change(summary, { target: { value: 'gpt-mock-mini' } });
    await next(2);
    await fireEvent.click(screen.getByRole('button', { name: 'Concluir' }));
    await waitFor(() => expect(finish_onboarding).toHaveBeenCalled());
    expect(vi.mocked(finish_onboarding).mock.calls[0][0]).toMatchObject({
      provider: 'codex',
      claude_model: 'opus',
      codex_model: 'gpt-mock-mini',
      codex_briefing_model: '',
    });
  });

  it('a_saved_model_outside_the_list_stays_selected', async () => {
    await withView((v) => {
      v.claude_model = 'claude-custom-1';
    });
    await openWizard();
    await next(2);
    expect(select('Claude Code', 'Modelo dos resumos').value).toBe('claude-custom-1');
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

  it('state_failure_shows_the_error_with_retry_instead_of_skipping_the_wizard', async () => {
    vi.mocked(onboarding_state).mockRejectedValueOnce(new Error('disk exploded'));
    render(App);
    expect(await screen.findByText('Algo deu errado: disk exploded')).toBeTruthy();
    expect(screen.queryByRole('navigation')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Tentar de novo' }));
    expect(await screen.findByRole('heading', { name: 'Detecção' })).toBeTruthy();
  });

  it('no_backend_falls_back_to_done', async () => {
    vi.mocked(onboarding_state).mockRejectedValueOnce(new Error(NO_BACKEND));
    render(App);
    expect(await screen.findByRole('navigation')).toBeTruthy();
  });

  it('failed_refresh_after_writing_keeps_the_view_and_toasts', async () => {
    await openWizard();
    await next();
    vi.mocked(onboarding_state).mockRejectedValueOnce(new Error('refresh broke'));
    await fireEvent.click(screen.getByRole('button', { name: 'Gravar hook' }));
    expect((await screen.findByRole('status')).textContent).toContain('refresh broke');
    expect(screen.getByRole('heading', { name: 'Hooks' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Gravar hook' })).toBeTruthy();
  });

  it('import_error_is_shown_once', async () => {
    await openWizard();
    await next(3);
    const handler = vi.mocked(onImportDone).mock.calls.at(-1)![0];
    handler({ counts: { sessions: 0, projects: 0, claude: 0, codex: 0 }, error: 'boom' });
    const text = (await screen.findByText(/Falha na importação/)).textContent ?? '';
    expect(text).toBe('Falha na importação: boom');
  });

  it('reopened_wizard_links_back_to_today', async () => {
    mock.reset({ delayMs: 20, onboardingDone: true });
    window.location.hash = '#/onboarding';
    render(App);
    const link = (await screen.findByRole('link', { name: 'Voltar para Hoje' })) as HTMLAnchorElement;
    expect(link.getAttribute('href')).toBe('#/');
  });

  it('settings_link_opens_the_wizard_as_settings_and_save_returns_to_today', async () => {
    mock.reset({ delayMs: 20, onboardingDone: true });
    render(App);
    const link = (await screen.findByRole('link', { name: /Ajustes/ })) as HTMLAnchorElement;
    expect(link.getAttribute('href')).toBe('#/settings');
    window.location.hash = '#/settings';
    expect(await screen.findByRole('heading', { level: 1, name: 'Ajustes' })).toBeTruthy();
    expect(screen.queryByRole('heading', { name: 'Primeira abertura' })).toBeNull();
    await next(4);
    expect(screen.queryByRole('button', { name: 'Concluir' })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Salvar' }));
    expect(await screen.findByRole('navigation')).toBeTruthy();
    expect(window.location.hash).toBe('#/');
    expect(finish_onboarding).toHaveBeenCalledTimes(1);
    expect((await screen.findByRole('status')).textContent).toContain('Ajustes salvos.');
  });

  it('first_run_keeps_its_title_and_finish_button', async () => {
    await openWizard();
    expect(screen.getByRole('heading', { level: 1, name: 'Primeira abertura' })).toBeTruthy();
    await next(4);
    expect(screen.getByRole('button', { name: 'Concluir' })).toBeTruthy();
  });

  it('remove_hook_shows_the_diff_and_only_removes_on_confirm', async () => {
    await withView((v) => {
      v.cards[0].hook_status = 'current';
      v.cards[0].diff = null;
      v.cards[0].can_write = false;
      v.cards[0].diff_remove = 'DIFF REMOVENDO';
    });
    await openWizard();
    await next();
    expect(screen.queryByText('DIFF REMOVENDO')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Remover hook' }));
    expect(screen.getByText('DIFF REMOVENDO')).toBeTruthy();
    expect(remove_hooks).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByRole('button', { name: 'Confirmar remoção' }));
    await waitFor(() => expect(remove_hooks).toHaveBeenCalledWith('claude'));
    expect((await screen.findByRole('status')).textContent).toContain('Hook removido.');
  });

  it('cancel_hides_the_remove_diff_without_removing', async () => {
    await withView((v) => {
      v.cards[0].hook_status = 'current';
      v.cards[0].diff = null;
      v.cards[0].can_write = false;
      v.cards[0].diff_remove = 'DIFF REMOVENDO';
    });
    await openWizard();
    await next();
    await fireEvent.click(screen.getByRole('button', { name: 'Remover hook' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Cancelar' }));
    expect(screen.queryByText('DIFF REMOVENDO')).toBeNull();
    expect(remove_hooks).not.toHaveBeenCalled();
  });

  it('no_remove_button_without_our_hooks', async () => {
    await openWizard();
    await next();
    expect(screen.queryByRole('button', { name: 'Remover hook' })).toBeNull();
  });

  it('written_hook_can_then_be_removed_in_the_mock', async () => {
    await openWizard();
    await next();
    const claude = screen.getByRole('heading', { name: 'Claude Code' }).closest('article')!;
    await fireEvent.click(within(claude as HTMLElement).getByRole('button', { name: 'Gravar hook' }));
    await fireEvent.click(await within(claude as HTMLElement).findByRole('button', { name: 'Remover hook' }));
    await fireEvent.click(within(claude as HTMLElement).getByRole('button', { name: 'Confirmar remoção' }));
    expect(await within(claude as HTMLElement).findByRole('button', { name: 'Gravar hook' })).toBeTruthy();
    await waitFor(() =>
      expect(within(claude as HTMLElement).queryByRole('button', { name: 'Remover hook' })).toBeNull(),
    );
  });
});

describe('language', () => {
  it('app_follows_the_backend_lang', async () => {
    mock.reset({ delayMs: 20, onboardingDone: false, lang: 'en' });
    render(App);
    await screen.findByRole('heading', { name: 'Detection' });
    expect(screen.getByRole('heading', { name: 'First run' })).toBeTruthy();
    expect(document.documentElement.lang).toBe('en');
  });
});
