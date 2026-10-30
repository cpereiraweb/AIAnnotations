// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../App.svelte';
import { mock } from '../lib/mock';
import { setLang } from '../lib/i18n';
import { ERR_BRIEFING_RUNNING, regenerate_briefing } from '../lib/api';

// Delegates to the real API (and so to the mock) unless a test overrides one call.
vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>();
  return { ...actual, regenerate_briefing: vi.fn(actual.regenerate_briefing) };
});

const writeText = vi.fn();

beforeEach(async () => {
  setLang('pt-BR');
  mock.reset({ delayMs: 20 });
  writeText.mockReset();
  writeText.mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
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
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    expect(writeText.mock.calls[0][0]).toMatch(/^cd '\/home\/dev\/Code\/loja virtual' && codex resume /);
    expect((await screen.findByRole('status')).textContent).toBe('comando copiado');
  });

  it('resume_toasts_when_the_clipboard_fails', async () => {
    writeText.mockRejectedValue(new Error('denied'));
    await openToday();
    const card = screen.getByText('loja-virtual · feat/pix').closest('article') as HTMLElement;
    await fireEvent.click(within(card).getByRole('button', { name: '▶ retomar (Codex)' }));
    expect((await screen.findByRole('status')).textContent).toBe('não foi possível copiar o comando');
  });

  it('ongoing_resume_copies_the_restart_prompt', async () => {
    await openToday();
    const row = screen.getByText('Checkout com Pix').closest('li') as HTMLElement;
    await fireEvent.click(within(row).getByRole('button', { name: '▶ retomar' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    expect(writeText).toHaveBeenCalledWith(
      "cd '/home/dev/Code/loja virtual' && claude '/ongoing-restart 2026-10-28-checkout-pix'",
    );
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
});
