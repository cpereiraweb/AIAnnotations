import { describe, expect, it, vi } from 'vitest';
import { MockBackend } from './mock';
import type { Badge, ItemKind, Page, ProjectCard, RunCard, TodayView } from './types';

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

  it('builds the resume command from the run', async () => {
    const m = new MockBackend({ now: NOW });
    const cmd = await m.call<string>('resume_command', { run_id: 5 });
    expect(cmd).toMatch(/^cd '\/home\/dev\/Code\/loja virtual' && codex resume /);
  });
});
