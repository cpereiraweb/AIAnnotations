// Hash router: `#/`, `#/projects`, `#/projects/:id`, `#/sessions`. Unknown hashes show Today.
export type Route =
  | { name: 'today' }
  | { name: 'projects' }
  | { name: 'project'; id: number }
  | { name: 'sessions' };

export function parseRoute(hash: string): Route {
  const path = hash.replace(/^#/, '').replace(/\/+$/, '');
  if (path === '/projects') return { name: 'projects' };
  if (path === '/sessions') return { name: 'sessions' };
  const project = /^\/projects\/(\d+)$/.exec(path);
  if (project) return { name: 'project', id: Number(project[1]) };
  return { name: 'today' };
}

export const router = $state<{ route: Route }>({
  route: parseRoute(typeof location === 'undefined' ? '' : location.hash),
});

if (typeof window !== 'undefined') {
  window.addEventListener('hashchange', () => {
    router.route = parseRoute(location.hash);
  });
}
