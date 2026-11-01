// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../App.svelte';
import { mock } from '../lib/mock';
import { setLang } from '../lib/i18n';
import { copy_resume_command, ERR_BRIEFING_RUNNING, regenerate_briefing, today } from '../lib/api';

// Delegates to the real API (and so to the mock) unless a test overrides one call.
vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>();
  return {
    ...actual,
    regenerate_briefing: vi.fn(actual.regenerate_briefing),
    today: vi.fn(actual.today),
    copy_resume_command: vi.fn(actual.copy_resume_command),
  };
});

beforeEach(async () => {
  setLang('pt-BR');
  mock.reset({ delayMs: 20 });
  window.location.hash = '#/';
});

afterEach(() => {
  cleanup();
  setLang('en');
});

async function openToday() {
  render(App);
  await screen.findByText(/^Precisa de você · \d+$/);
}

describe('Today', () => {
  it('renders_sections_with_counts_from_mock', async () => {
    await openToday();
    expect(screen.getByRole('heading', { name: /^Briefing de / })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Em andamento · 3' })).toBeTruthy();
    expect(screen.getByRole('button', { name: /Parados há mais de 30 dias · 1/ })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Precisa de você · 7' })).toBeTruthy();
    expect(screen.getByRole('button', { name: /Parado há mais tempo · 1/ })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Próximos passos sugeridos' })).toBeTruthy();
    expect(screen.getByText('histórico: 42/120 resumidas')).toBeTruthy();
    // Counters of the group header.
    expect(screen.getByText('1 interrompida · 1 lembrete · 1 worktree')).toBeTruthy();
    // Navigation and the search button of the layout.
    expect(screen.getByRole('link', { name: '☀ Hoje' }).getAttribute('aria-current')).toBe('page');
    expect(screen.getByRole('button', { name: /Ctrl\+K/ })).toBeTruthy();
  });

  it('expands only the group the backend marks as expanded', async () => {
    await openToday();
    const groups = document.querySelectorAll('[data-inbox-group]');
    expect([...groups].map((g) => g.getAttribute('data-inbox-group'))).toEqual(['expanded', 'expanded', 'collapsed']);
    const collapsed = within(groups[2] as HTMLElement).getByRole('button', { name: /infra-scripts/ });
    expect(collapsed.getAttribute('aria-expanded')).toBe('false');
    expect(within(groups[2] as HTMLElement).queryByText('já vi')).toBeNull();
    await fireEvent.click(collapsed);
    expect(within(groups[2] as HTMLElement).getByText('já vi')).toBeTruthy();
  });

  it('dismiss_removes_item', async () => {
    await openToday();
    expect(screen.getByText('#199 Ícone da bandeja no macOS')).toBeTruthy();
    const card = screen.getByText('#199 Ícone da bandeja no macOS').closest('article') as HTMLElement;
    await fireEvent.click(within(card).getByRole('button', { name: 'já vi' }));
    await waitFor(() => expect(screen.queryByText('#199 Ícone da bandeja no macOS')).toBeNull());
    expect(screen.getByRole('heading', { name: 'Precisa de você · 6' })).toBeTruthy();
  });

  it('dismiss_group_sends_the_section_of_the_group', async () => {
    await openToday();
    // The older section holds infra-scripts (feat/monitoring); the recent one also has infra-scripts (dirty).
    await fireEvent.click(screen.getByRole('button', { name: /Parado há mais tempo · 1/ }));
    const older = screen.getByRole('button', { name: /Parado há mais tempo/ }).parentElement as HTMLElement;
    await fireEvent.click(within(older).getByRole('button', { name: 'já vi tudo' }));
    await waitFor(() => expect(screen.queryByRole('button', { name: /Parado há mais tempo/ })).toBeNull());
    // The recent group of the same project is untouched.
    expect(screen.getByRole('heading', { name: 'Precisa de você · 7' })).toBeTruthy();
  });

  it('resume_copies_command_and_toasts', async () => {
    await openToday();
    const card = screen.getByText('loja-virtual · feat/pix').closest('article') as HTMLElement;
    await fireEvent.click(within(card).getByRole('button', { name: '▶ retomar (Codex)' }));
    // The backend builds the command and writes the clipboard itself.
    expect((await screen.findByRole('status')).textContent).toBe('comando copiado');
    expect(mock.clipboard).toMatch(/^cd '\/home\/dev\/Code\/loja virtual' && codex resume /);
  });

  it('resume_toasts_when_the_copy_fails', async () => {
    vi.mocked(copy_resume_command).mockRejectedValueOnce('clipboard unavailable');
    await openToday();
    const card = screen.getByText('loja-virtual · feat/pix').closest('article') as HTMLElement;
    await fireEvent.click(within(card).getByRole('button', { name: '▶ retomar (Codex)' }));
    expect((await screen.findByRole('status')).textContent).toBe('não foi possível copiar o comando');
    expect(mock.clipboard).toBeNull();
  });

  it('ongoing_resume_copies_the_restart_prompt', async () => {
    await openToday();
    const row = screen.getByText('Checkout com Pix').closest('li') as HTMLElement;
    await fireEvent.click(within(row).getByRole('button', { name: '▶ retomar' }));
    expect((await screen.findByRole('status')).textContent).toBe('comando copiado');
    expect(mock.clipboard).toBe("cd '/home/dev/Code/loja virtual' && claude '/ongoing-restart 2026-10-28-checkout-pix'");
  });

  it('regenerate_shows_generating_then_clears_on_reload', async () => {
    await openToday();
    await fireEvent.click(screen.getByRole('button', { name: 'regerar' }));
    expect(await screen.findByText('gerando…')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'regerar' })).toBeNull();
    await waitFor(() => expect(screen.queryByText('gerando…')).toBeNull());
    expect(screen.getByRole('button', { name: 'regerar' })).toBeTruthy();
  });

  it('regenerate_toasts_when_already_running', async () => {
    // Another process holds the lock: the backend rejects with the known code.
    vi.mocked(regenerate_briefing).mockRejectedValueOnce(ERR_BRIEFING_RUNNING);
    await openToday();
    await fireEvent.click(screen.getByRole('button', { name: 'regerar' }));
    expect((await screen.findByRole('status')).textContent).toBe('O briefing já está sendo gerado.');
    // The generating state stays until the next reload (the running generation will emit "changed").
    expect(screen.getByText('gerando…')).toBeTruthy();
  });

  it('regenerate_goes_back_when_it_fails_for_another_reason', async () => {
    vi.mocked(regenerate_briefing).mockRejectedValueOnce('boom');
    await openToday();
    await fireEvent.click(screen.getByRole('button', { name: 'regerar' }));
    expect((await screen.findByRole('status')).textContent).toBe('Algo deu errado: boom');
    await waitFor(() => expect(screen.queryByText('gerando…')).toBeNull());
    expect(screen.getByRole('button', { name: 'regerar' })).toBeTruthy();
  });

  it('generating_briefing_offers_regenerate_when_nothing_runs', async () => {
    // A `generating` row no generation is working on (e.g. the app was closed mid-way).
    const actual = await vi.importActual<typeof import('../lib/api')>('../lib/api');
    vi.mocked(today).mockImplementation(async () => {
      const v = await actual.today();
      return { ...v, briefing: v.briefing && { ...v.briefing, status: 'generating', body: null }, briefing_running: false };
    });
    try {
      await openToday();
      expect(screen.getByText('gerando…')).toBeTruthy();
      expect(screen.getByRole('button', { name: 'regerar' })).toBeTruthy();
    } finally {
      vi.mocked(today).mockImplementation(actual.today);
    }
  });

  it('running_generation_hides_regenerate', async () => {
    const actual = await vi.importActual<typeof import('../lib/api')>('../lib/api');
    vi.mocked(today).mockImplementation(async () => {
      const v = await actual.today();
      return { ...v, briefing: v.briefing && { ...v.briefing, status: 'generating', body: null }, briefing_running: true };
    });
    try {
      await openToday();
      expect(screen.getByText('gerando…')).toBeTruthy();
      expect(screen.queryByRole('button', { name: 'regerar' })).toBeNull();
    } finally {
      vi.mocked(today).mockImplementation(actual.today);
    }
  });

  it('empty_db_shows_import_button', async () => {
    mock.reset({ empty: true, delayMs: 20 });
    render(App);
    expect(await screen.findByText('Nenhuma sessão ainda.')).toBeTruthy();
    expect(screen.getByText('Nada pendente. Bom trabalho.')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Importar histórico' }));
    expect((await screen.findByRole('status')).textContent).toBe('Importando o histórico…');
    // The import finishes and the screen reloads with the sessions.
    await waitFor(() => expect(screen.queryByText('Nenhuma sessão ainda.')).toBeNull());
  });
});

describe('router', () => {
  it('the tray "Hoje" item navigates to Today', async () => {
    window.location.hash = '#/projects';
    render(App);
    await waitFor(() => expect(screen.getByRole('link', { name: '▦ Projetos' }).getAttribute('aria-current')).toBe('page'));
    // The tray emits "navigate" with the hash.
    mock.navigate('#/');
    expect(await screen.findByText(/^Precisa de você · \d+$/)).toBeTruthy();
    expect(window.location.hash).toBe('#/');
  });

  it('shows the placeholders for the other routes and marks the nav', async () => {
    window.location.hash = '#/projects';
    render(App);
    await waitFor(() => expect(screen.getByRole('link', { name: '▦ Projetos' }).getAttribute('aria-current')).toBe('page'));
    expect(screen.queryByText(/Precisa de você/)).toBeNull();
    window.location.hash = '#/sessions';
    await waitFor(() => expect(screen.getByRole('link', { name: '☰ Sessões' }).getAttribute('aria-current')).toBe('page'));
    window.location.hash = '#/projects/7';
    await waitFor(() => expect(screen.getByRole('link', { name: '▦ Projetos' }).getAttribute('aria-current')).toBe('page'));
  });

  it('today_hooks_banner_points_to_settings', async () => {
    const actual = await vi.importActual<typeof import('../lib/api')>('../lib/api');
    vi.mocked(today).mockImplementation(async () => ({ ...(await actual.today()), hooks_need_update: true }));
    try {
      await openToday();
      const link = screen.getByRole('link', { name: /Os hooks precisam ser regravados/ });
      expect(link.getAttribute('href')).toBe('#/settings');
    } finally {
      vi.mocked(today).mockImplementation(actual.today);
    }
  });

  it('no_hooks_banner_by_default', async () => {
    await openToday();
    expect(screen.queryByText(/Os hooks precisam ser regravados/)).toBeNull();
  });

  it('login_notice', async () => {
    const actual = await vi.importActual<typeof import('../lib/api')>('../lib/api');
    vi.mocked(today).mockImplementation(async () => ({
      ...(await actual.today()),
      ai_attention: { kind: 'login', tool: 'claude' },
    }));
    try {
      await openToday();
      expect(
        screen.getByText(
          'Faça login no Claude Code (rode `claude` no terminal) para voltar a gerar resumos e o briefing.',
        ),
      ).toBeTruthy();
    } finally {
      vi.mocked(today).mockImplementation(actual.today);
    }
  });

  it('briefing_failed_notice_shows_the_detail_once', async () => {
    const actual = await vi.importActual<typeof import('../lib/api')>('../lib/api');
    vi.mocked(today).mockImplementation(async () => {
      const v = await actual.today();
      return {
        ...v,
        briefing: v.briefing && { ...v.briefing, status: 'failed' },
        ai_attention: { kind: 'briefing_failed', detail: 'boom happened' },
      };
    });
    try {
      await openToday();
      expect(screen.getAllByText(/Não foi possível gerar o briefing agora\./)).toHaveLength(1);
      expect(screen.getByText(/boom happened/)).toBeTruthy();
    } finally {
      vi.mocked(today).mockImplementation(actual.today);
    }
  });

  it('no_ai_notice_by_default', async () => {
    await openToday();
    expect(screen.queryByText(/Faça login no/)).toBeNull();
  });
});
