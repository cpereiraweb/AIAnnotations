import { describe, expect, it, vi } from 'vitest';
import { MockBackend } from './mock';
import type { Badge, ItemKind, OnboardingView, Page, ProjectCard, RunCard, TodayView } from './types';

const NOW = Date.parse('2026-10-30T14:00:00-03:00');

function items(v: TodayView) {
  return [...v.inbox.recent, ...v.inbox.older].flatMap((g) => g.items);
}

describe('MockBackend', () => {
  it('has every badge and kind, recent and stale ongoings, a done briefing and backfill', async () => {
    const m = new MockBackend({ now: NOW });
    const v = await m.call<TodayView>('today', {});
    const badges = new Set(items(v).map((i) => i.badge));
    const kinds = new Set(items(v).map((i) => i.kind));
    const allBadges: Badge[] = ['interrupted', 'half_done', 'stale_branch', 'uncommitted_changes', 'worktree', 'reminder', 'ongoing'];
    const allKinds: ItemKind[] = ['interrupted', 'stale_branch', 'dirty', 'worktree', 'reminder', 'ongoing'];
    expect([...badges].sort()).toEqual([...allBadges].sort());
    expect([...kinds].sort()).toEqual([...allKinds].sort());
    expect(v.inbox.older.length).toBeGreaterThan(0);
    expect(v.inbox.recent[0].expanded).toBe(true);
    expect(v.ongoings_recent.length).toBeGreaterThan(0);
    expect(v.ongoings_stale.length).toBeGreaterThan(0);
    expect(v.briefing?.status).toBe('done');
    expect(v.next_steps.length).toBeGreaterThan(0);
    expect(v.backfill).not.toBeNull();
    expect(v.is_empty_db).toBe(false);
    for (const i of items(v).filter((i) => i.session_id !== null)) {
      expect(v.inbox_last_runs[String(i.session_id)]).toBeDefined();
    }
    const projects = await m.call<ProjectCard[]>('projects', {});
    expect(projects).toHaveLength(3);
  });

  it('has a /clear chain and hidden runs only with include_hidden', async () => {
    const m = new MockBackend({ now: NOW });
    const visible = await m.call<Page<RunCard>>('project_runs', { id: 1, include_hidden: false, page: 1 });
    const all = await m.call<Page<RunCard>>('project_runs', { id: 1, include_hidden: true, page: 1 });
    expect(visible.items.every((r) => r.hidden_label === null)).toBe(true);
    expect(all.total).toBeGreaterThan(visible.total);
    const roots = visible.items.filter((r) => r.root_session_id === 10);
    expect(new Set(roots.map((r) => r.session_id)).size).toBeGreaterThan(1);
  });

  it('dismiss and dismiss_group change the fixtures without firing onChanged (as the real commands)', async () => {
    const m = new MockBackend({ now: NOW });
    const changed = vi.fn();
    m.onChanged(changed);
    const first = items(await m.call<TodayView>('today', {}))[0];
    await m.call('dismiss', { key: first.key, state_hash: first.state_hash });
    let rest = items(await m.call<TodayView>('today', {}));
    expect(rest.some((i) => i.key === first.key)).toBe(false);
    const n = await m.call<number>('dismiss_group', { project_id: rest[0].project_id, older: false });
    expect(n).toBeGreaterThan(0);
    const gone = rest[0].project_id;
    const v = await m.call<TodayView>('today', {});
    expect(v.inbox.recent.some((g) => g.project_id === gone)).toBe(false);
    rest = items(v);
    expect(changed).not.toHaveBeenCalled();
  });

  it('dismiss_group only clears the section the button sits in', async () => {
    const m = new MockBackend({ now: NOW });
    const v = await m.call<TodayView>('today', {});
    const project = v.inbox.older[0].project_id;
    expect(v.inbox.recent.some((g) => g.project_id === project)).toBe(true);

    await m.call('dismiss_group', { project_id: project, older: true });
    let after = await m.call<TodayView>('today', {});
    expect(after.inbox.older.some((g) => g.project_id === project)).toBe(false);
    expect(after.inbox.recent.some((g) => g.project_id === project)).toBe(true);

    m.reset({ now: NOW });
    await m.call('dismiss_group', { project_id: project, older: false });
    after = await m.call<TodayView>('today', {});
    expect(after.inbox.recent.some((g) => g.project_id === project)).toBe(false);
    expect(after.inbox.older.some((g) => g.project_id === project)).toBe(true);
  });

  it('regenerate rejects with already_running while generating', async () => {
    vi.useFakeTimers();
    try {
      const m = new MockBackend({ now: NOW, delayMs: 100 });
      await m.call('regenerate_briefing', {});
      expect((await m.call<TodayView>('today', {})).briefing?.status).toBe('generating');
      await expect(m.call('regenerate_briefing', {})).rejects.toBe('already_running');
      vi.advanceTimersByTime(100);
      expect((await m.call<TodayView>('today', {})).briefing?.status).toBe('done');
    } finally {
      vi.useRealTimers();
    }
  });

  it('today starts the generation of a generating briefing once, like the app', async () => {
    vi.useFakeTimers();
    try {
      const m = new MockBackend({ now: NOW, empty: true, delayMs: 100 });
      const first = await m.call<TodayView>('today', {});
      expect(first.briefing?.status).toBe('generating');
      expect(first.briefing_running).toBe(true);
      // A second load while it runs does not start another one.
      expect((await m.call<TodayView>('today', {})).briefing_running).toBe(true);
      vi.advanceTimersByTime(100);
      const done = await m.call<TodayView>('today', {});
      expect(done.briefing?.status).toBe('done');
      expect(done.briefing_running).toBe(false);
      expect(done.briefing?.body).toBe('Nenhuma sessão desde o último briefing.');
    } finally {
      vi.useRealTimers();
    }
  });

  it('import_history fills an empty database', async () => {
    vi.useFakeTimers();
    try {
      const m = new MockBackend({ now: NOW, empty: true, delayMs: 100 });
      expect((await m.call<TodayView>('today', {})).is_empty_db).toBe(true);
      await m.call('import_history', {});
      await expect(m.call('import_history', {})).rejects.toBe('import_running');
      vi.advanceTimersByTime(100);
      const v = await m.call<TodayView>('today', {});
      expect(v.is_empty_db).toBe(false);
      expect(v.backfill?.[0]).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it('resumes the run, copying the command', async () => {
    const m = new MockBackend({ now: NOW });
    expect(m.clipboard).toBeNull();
    const out = await m.call<{ command: string; opened: string }>('resume_run', { run_id: 5 });
    expect(out.command).toMatch(/^cd '\/home\/dev\/Code\/loja virtual' && codex resume /);
    expect(out.opened).toBe('ran');
    expect(m.clipboard).toBe(out.command);
    await expect(m.call('resume_run', { run_id: 999 })).rejects.toMatch(/run not found/);
  });

  it('save_settings patches the wizard state and refuses before it is done', async () => {
    const pending = new MockBackend({ now: NOW, onboardingDone: false });
    await expect(pending.call('save_settings', { patch: { claude_model: 'opus' } })).rejects.toBe('onboarding pending');

    const m = new MockBackend({ now: NOW, lang: 'en' });
    await m.call('save_settings', { patch: { claude_model: 'opus' } });
    const v = await m.call<OnboardingView>('onboarding_state', {});
    expect(v.claude_model).toBe('opus');
    expect(m.choices?.claude_model).toBe('opus');

    await m.call('save_settings', { patch: { lang: 'pt-BR' } });
    let l = await m.call<OnboardingView>('onboarding_state', {});
    expect([l.lang, l.lang_choice]).toEqual(['pt-BR', 'pt-BR']);
    await m.call('save_settings', { patch: { lang: null } });
    l = await m.call<OnboardingView>('onboarding_state', {});
    expect([l.lang, l.lang_choice]).toEqual(['en', null]);
  });

  it('set_summaries_paused toggles the pause and shows on Today', async () => {
    const m = new MockBackend({ now: NOW });
    expect(await m.call('set_summaries_paused', { paused: true })).toBe(true);
    expect((await m.call<TodayView>('today', {})).summaries_paused).toBe(true);
    expect(await m.call('set_summaries_paused', { paused: false })).toBe(false);
    expect((await m.call<TodayView>('today', {})).summaries_paused).toBe(false);
    const pending = new MockBackend({ now: NOW, onboardingDone: false });
    expect(await pending.call('set_summaries_paused', { paused: false })).toBe(true);
  });
});
