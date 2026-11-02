// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../App.svelte';
import * as api from '../lib/api';
import { setLang } from '../lib/i18n';
import { mock } from '../lib/mock';

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>();
  return { ...actual, sessions: vi.fn(actual.sessions) };
});

beforeEach(() => {
  setLang('pt-BR');
  mock.reset({ delayMs: 20 });
  window.location.hash = '#/sessions';
});

afterEach(() => {
  cleanup();
  setLang('en');
  vi.mocked(api.sessions).mockClear();
});

const cards = () => [...document.querySelectorAll('[data-run]')];

describe('Sessions', () => {
  it('sessions_filters_update_hash_and_reload', async () => {
    render(App);
    await waitFor(() => expect(cards().length).toBeGreaterThan(0));
    const all = cards().length;
    expect(cards().some((c) => c.textContent?.includes('Claude'))).toBe(true);

    await fireEvent.change(screen.getByLabelText('Ferramenta'), { target: { value: 'codex' } });
    expect(window.location.hash).toBe('#/sessions?tool=codex');
    await waitFor(() => expect(api.sessions).toHaveBeenLastCalledWith({ tool: 'codex', project_id: null, kind: null }, 1));
    await waitFor(() => expect(cards().length).toBeLessThan(all));
    expect(cards().some((c) => c.textContent?.includes('Claude'))).toBe(false);

    await fireEvent.change(screen.getByLabelText('Tipo'), { target: { value: 'talk' } });
    expect(window.location.hash).toBe('#/sessions?tool=codex&kind=talk');
    await waitFor(() => expect(api.sessions).toHaveBeenLastCalledWith({ tool: 'codex', project_id: null, kind: 'talk' }, 1));
    await waitFor(() => expect(cards().length).toBeGreaterThan(0));
    expect(cards().every((c) => within(c as HTMLElement).queryByText('💬 só conversa'))).toBe(true);
  });

  it('a filter change resets the page to 1', async () => {
    window.location.hash = '#/sessions?page=2&tool=claude';
    render(App);
    await waitFor(() => expect(api.sessions).toHaveBeenCalledWith({ tool: 'claude', project_id: null, kind: null }, 2));
    await fireEvent.change(screen.getByLabelText('Projeto'), { target: { value: '1' } });
    expect(window.location.hash).toBe('#/sessions?tool=claude&project=1');
    await waitFor(() => expect(api.sessions).toHaveBeenLastCalledWith({ tool: 'claude', project_id: 1, kind: null }, 1));
  });

  it('shows the empty state when nothing matches', async () => {
    window.location.hash = '#/sessions?project=3&kind=changed&tool=codex';
    render(App);
    expect(await screen.findByText('Nenhuma sessão encontrada.')).toBeTruthy();
  });

  it('paginates through the hash', async () => {
    vi.mocked(api.sessions).mockImplementation(async (_f, page) => ({
      items: (await mock.call<{ items: never[] }>('sessions', { filter: {}, page: 1 })).items.slice(0, 2),
      page,
      pages: 3,
      total: 90,
    }));
    window.location.hash = '#/sessions?page=2';
    render(App);
    expect(await screen.findByText('página 2 de 3')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'mais antigas →' }));
    expect(window.location.hash).toBe('#/sessions?page=3');
    await fireEvent.click(await screen.findByRole('button', { name: '← mais recentes' }));
    await fireEvent.click(await screen.findByRole('button', { name: '← mais recentes' }));
    expect(window.location.hash).toBe('#/sessions');
    vi.mocked(api.sessions).mockRestore();
  });
});

describe('Project detail', () => {
  it('clear_chain_groups_runs', async () => {
    window.location.hash = '#/projects/1';
    render(App);
    const chain = await waitFor(() => {
      const el = document.querySelector('[data-chain]');
      if (!el) throw new Error('no chain yet');
      return el as HTMLElement;
    });
    expect(within(chain).getByText('continuação via /clear · Worker em segundo plano')).toBeTruthy();
    expect(chain.querySelectorAll('[data-run]')).toHaveLength(3);
    // Nested cards do not repeat their own title.
    expect(within(chain).queryByText('Worker: ajustes da revisão')).toBeNull();
    // A run outside the chain keeps its title and is not grouped.
    const outside = [...document.querySelectorAll('[data-run]')].filter((c) => !chain.contains(c));
    expect(outside.length).toBeGreaterThan(0);
    expect(document.querySelectorAll('[data-chain]')).toHaveLength(1);
  });

  it('hidden runs show only with the checkbox, dimmed and labeled', async () => {
    window.location.hash = '#/projects/1';
    render(App);
    await screen.findByText('mostrar sessões ocultas');
    await waitFor(() => expect(cards().length).toBeGreaterThan(0));
    const before = cards().length;
    expect(document.querySelectorAll('.opacity-70')).toHaveLength(0);
    await fireEvent.click(screen.getByLabelText('mostrar sessões ocultas'));
    expect(window.location.hash).toBe('#/projects/1?hidden=1');
    await waitFor(() => expect(cards().length).toBeGreaterThan(before));
    expect(document.querySelectorAll('.opacity-70').length).toBeGreaterThan(0);
    expect(screen.getByText('vazia')).toBeTruthy();
  });

  it('ongoings resume copies the command', async () => {
    window.location.hash = '#/projects/2';
    render(App);
    expect(await screen.findByRole('heading', { name: /^🔄 Em andamento · \d+$/ })).toBeTruthy();
    await fireEvent.click(screen.getAllByRole('button', { name: '▶ retomar' })[0]);
    await waitFor(() => expect(mock.clipboard).toMatch(/Retome o ongoing /));
  });
});

describe('Projects', () => {
  it('lists the cards with badge, chips and summary', async () => {
    window.location.hash = '#/projects';
    render(App);
    const card = (await screen.findByText('loja-virtual')).closest('[data-project]') as HTMLElement;
    expect(within(card).getByText(/^🔄 \d+$/)).toBeTruthy();
    expect(within(card).getByText('/home/dev/Code/loja virtual')).toBeTruthy();
    expect(within(card).getByText('https://github.com/exemplo/loja-virtual.git')).toBeTruthy();
    expect(card.textContent).toMatch(/Codex [✎💬]/);
    await fireEvent.click(await within(card).findByRole('button', { name: 'Abrir no VS Code' }));
  });

  it('shows the empty state', async () => {
    mock.reset({ empty: true, delayMs: 20 });
    window.location.hash = '#/projects';
    render(App);
    expect(await screen.findByText('Nenhum projeto registrado ainda.')).toBeTruthy();
  });
});
