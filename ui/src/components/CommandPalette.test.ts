// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../lib/api';
import { setLang } from '../lib/i18n';
import { mock } from '../lib/mock';
import CommandPalette from './CommandPalette.svelte';

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>();
  return { ...actual, search: vi.fn(actual.search) };
});

beforeEach(() => {
  setLang('pt-BR');
  mock.reset({ delayMs: 20 });
  window.location.hash = '#/';
});

afterEach(() => {
  cleanup();
  setLang('en');
  vi.mocked(api.search).mockClear();
});

const box = () => screen.getByPlaceholderText('Buscar em projetos, branches e resumos…') as HTMLInputElement;

async function openWithShortcut() {
  render(CommandPalette);
  await fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
  return box();
}

describe('CommandPalette', () => {
  it('palette_debounces_and_navigates', async () => {
    const input = await openWithShortcut();
    await fireEvent.input(input, { target: { value: 'w' } });
    await fireEvent.input(input, { target: { value: 'wor' } });
    await fireEvent.input(input, { target: { value: 'worker' } });
    // Nothing is sent before the 250 ms pause...
    expect(api.search).not.toHaveBeenCalled();
    // ...and then only the last text is.
    await waitFor(() => expect(api.search).toHaveBeenCalledTimes(1));
    expect(api.search).toHaveBeenCalledWith('worker');

    const options = await screen.findAllByRole('option');
    expect(options.length).toBeGreaterThan(0);
    expect(options[0].textContent).toContain('aiannotations');
    expect(options[0].textContent).toContain('feat/worker');

    // Arrow keys move the selection; Enter opens the project.
    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    await fireEvent.keyDown(input, { key: 'ArrowUp' });
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(window.location.hash).toBe('#/projects/1');
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('click on a result navigates to the project', async () => {
    const input = await openWithShortcut();
    await fireEvent.input(input, { target: { value: 'pix' } });
    const option = await screen.findByRole('option', {}, { timeout: 1000 });
    await fireEvent.click(option.querySelector('button')!);
    expect(window.location.hash).toBe('#/projects/2');
  });

  it('caps the results at 12', async () => {
    vi.mocked(api.search).mockResolvedValueOnce(
      Array.from({ length: 12 }, (_, i) => ({
        run_id: i, ai_session_id: i, project_id: 1, project_name: `p${i}`, tool: 'claude' as const,
        branch: null, title: null, summary: null, next_step: null, started_at_ms: Date.now(),
      })),
    );
    const input = await openWithShortcut();
    await fireEvent.input(input, { target: { value: 'p' } });
    expect(await screen.findAllByRole('option')).toHaveLength(12);
  });

  it('says so when nothing is found', async () => {
    const input = await openWithShortcut();
    await fireEvent.input(input, { target: { value: 'zzzzqq' } });
    expect(await screen.findByText('Nada encontrado.')).toBeTruthy();
  });

  it('opens with the open-palette event and closes with Esc or a click outside', async () => {
    render(CommandPalette);
    expect(screen.queryByRole('dialog')).toBeNull();
    window.dispatchEvent(new CustomEvent('open-palette'));
    await screen.findByRole('dialog');
    await fireEvent.keyDown(box(), { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();

    window.dispatchEvent(new CustomEvent('open-palette'));
    await screen.findByRole('dialog');
    await fireEvent.click(document.querySelector('[data-palette-backdrop]')!);
    expect(screen.queryByRole('dialog')).toBeNull();
    // Clicking inside does not close it.
    window.dispatchEvent(new CustomEvent('open-palette'));
    await fireEvent.click(await screen.findByRole('dialog'));
    expect(screen.getByRole('dialog')).toBeTruthy();
  });

  it('Cmd+K also opens it', async () => {
    render(CommandPalette);
    await fireEvent.keyDown(window, { key: 'K', metaKey: true });
    expect(await screen.findByRole('dialog')).toBeTruthy();
  });
});
