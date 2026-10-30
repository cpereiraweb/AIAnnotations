import { afterEach, describe, expect, it, vi } from 'vitest';
import { NO_BACKEND, dismiss_group, onChanged, regenerate_briefing, today } from './api';

describe('api outside Tauri', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('uses the mock in test mode and forwards dismiss_group(project_id, older)', async () => {
    const v = await today();
    const project = v.inbox.older[0].project_id;
    await dismiss_group(project, true);
    const after = await today();
    expect(after.inbox.older.some((g) => g.project_id === project)).toBe(false);
    expect(after.inbox.recent.some((g) => g.project_id === project)).toBe(true);
  });

  it('onChanged fires when the backend writes in the background', async () => {
    const changed = vi.fn();
    const stop = onChanged(changed);
    await vi.waitFor(async () => {
      await regenerate_briefing().catch(() => {});
      expect(changed).toHaveBeenCalled();
    });
    stop();
  });

  it('fails clearly instead of serving fixtures outside the mock mode', async () => {
    vi.stubEnv('MODE', 'production');
    await expect(today()).rejects.toThrow(NO_BACKEND);
  });
});
