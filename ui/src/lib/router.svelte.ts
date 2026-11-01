// Hash router: `#/`, `#/projects`, `#/projects/:id`, `#/sessions`, `#/onboarding`, each with an optional
// `?query` (filters and page live there). Unknown hashes show Today. `#/settings` is the wizard
// reopened after the first run (it then shows as "Ajustes").
export type Route =
  | { name: 'today' }
  | { name: 'projects' }
  | { name: 'project'; id: number }
  | { name: 'sessions' }
  | { name: 'onboarding' };

export function parseRoute(hash: string): Route {
  const path = hash.replace(/^#/, '').replace(/\?.*$/, '').replace(/\/+$/, '');
  if (path === '/projects') return { name: 'projects' };
  if (path === '/sessions') return { name: 'sessions' };
  if (path === '/onboarding' || path === '/settings') return { name: 'onboarding' };
  const project = /^\/projects\/(\d+)$/.exec(path);
  if (project) return { name: 'project', id: Number(project[1]) };
  return { name: 'today' };
}

/** The `?key=value` part of the hash. */
export function parseQuery(hash: string): URLSearchParams {
  const i = hash.indexOf('?');
  return new URLSearchParams(i < 0 ? '' : hash.slice(i + 1));
}

const read = () => (typeof location === 'undefined' ? '' : location.hash);

export const router = $state<{ route: Route; query: URLSearchParams }>({
  route: parseRoute(read()),
  query: parseQuery(read()),
});

if (typeof window !== 'undefined') {
  window.addEventListener('hashchange', () => {
    router.route = parseRoute(location.hash);
    router.query = parseQuery(location.hash);
  });
}

/** Replaces the query of the current route: empty values and `null` drop the key. */
export function setQuery(params: Record<string, string | number | null | undefined>): void {
  const q = new URLSearchParams(router.query);
  for (const [key, value] of Object.entries(params)) {
    if (value === null || value === undefined || value === '') q.delete(key);
    else q.set(key, String(value));
  }
  const path = location.hash.replace(/^#/, '').replace(/\?.*$/, '') || '/';
  const qs = q.toString();
  location.hash = qs ? `#${path}?${qs}` : `#${path}`;
}
