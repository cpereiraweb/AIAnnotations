import { describe, expect, it } from 'vitest';
import { parseRoute } from './router.svelte';

describe('parseRoute', () => {
  it('maps the hash to a route', () => {
    expect(parseRoute('')).toEqual({ name: 'today' });
    expect(parseRoute('#/')).toEqual({ name: 'today' });
    expect(parseRoute('#/projects')).toEqual({ name: 'projects' });
    expect(parseRoute('#/projects/12')).toEqual({ name: 'project', id: 12 });
    expect(parseRoute('#/sessions')).toEqual({ name: 'sessions' });
    expect(parseRoute('#/onboarding')).toEqual({ name: 'onboarding' });
    expect(parseRoute('#/settings')).toEqual({ name: 'settings' });
  });

  it('falls back to Today for anything unknown', () => {
    expect(parseRoute('#/nope')).toEqual({ name: 'today' });
    expect(parseRoute('#/projects/abc')).toEqual({ name: 'today' });
  });
});
