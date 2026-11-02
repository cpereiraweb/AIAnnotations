// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { open_project_with } from '../lib/api';
import { resetAppsState } from '../lib/apps.svelte';
import { setLang } from '../lib/i18n';
import { mock } from '../lib/mock';
import { toastState } from '../lib/toast.svelte';
import OpenWith from './OpenWith.svelte';

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>();
  return { ...actual, open_project_with: vi.fn(actual.open_project_with) };
});

beforeEach(async () => {
  setLang('pt-BR');
  resetAppsState();
  toastState.message = '';
  const actual = await vi.importActual<typeof import('../lib/api')>('../lib/api');
  vi.mocked(open_project_with).mockReset().mockImplementation(actual.open_project_with);
  mock.reset({ delayMs: 20 });
});

afterEach(() => {
  cleanup();
  setLang('en');
});

describe('OpenWith', () => {
  it('open_with_has_no_default_button', async () => {
    render(OpenWith, { projectId: 1 });
    await screen.findByRole('combobox');
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('open_with_select_lists_all_editors_default_first', async () => {
    render(OpenWith, { projectId: 1 });
    const select = (await screen.findByRole('combobox')) as HTMLSelectElement;
    const labels = Array.from(select.options).map((o) => o.textContent);
    expect(labels[0]).toBe('abrir com…');
    expect(labels[1]).toBe('VS Code (padrão)');
    expect(labels).toContain('Cursor');
    expect(labels).toContain('Kitty');
    await fireEvent.change(select, { target: { value: 'vscode' } });
    expect(open_project_with).toHaveBeenCalledWith(1, 'vscode');
    await waitFor(() => expect(select.value).toBe(''));
  });

  it('open_with_select_opens_a_terminal_and_resets', async () => {
    render(OpenWith, { projectId: 1 });
    const select = (await screen.findByRole('combobox')) as HTMLSelectElement;
    await fireEvent.change(select, { target: { value: 'kitty' } });
    expect(open_project_with).toHaveBeenCalledWith(1, 'kitty');
    await waitFor(() => expect(select.value).toBe(''));
  });

  it('open_with_toasts_the_error', async () => {
    vi.mocked(open_project_with).mockRejectedValueOnce('app not found');
    render(OpenWith, { projectId: 1 });
    const select = (await screen.findByRole('combobox')) as HTMLSelectElement;
    await fireEvent.change(select, { target: { value: 'vscode' } });
    await waitFor(() => expect(toastState.message).toBe('O app não foi encontrado.'));
  });
});
