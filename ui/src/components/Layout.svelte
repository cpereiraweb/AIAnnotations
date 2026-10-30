<script lang="ts">
  import type { Snippet } from 'svelte';
  import { t, type Key } from '../lib/i18n';
  import { router } from '../lib/router.svelte';
  import CommandPalette from './CommandPalette.svelte';
  import Toast from './Toast.svelte';

  let { children }: { children: Snippet } = $props();

  const links: { href: string; name: string; label: Key }[] = [
    { href: '#/', name: 'today', label: 'nav.today' },
    { href: '#/projects', name: 'projects', label: 'nav.projects' },
    { href: '#/sessions', name: 'sessions', label: 'nav.sessions' },
  ];

  // The project page belongs to the "Projetos" entry.
  const current = $derived(router.route.name === 'project' ? 'projects' : router.route.name);

  // The command palette listens for this event (and for Ctrl+K / ⌘K).
  const openPalette = () => window.dispatchEvent(new CustomEvent('open-palette'));
</script>

<div class="min-h-screen bg-stone-50 text-[13px] leading-normal text-stone-800 antialiased dark:bg-stone-950 dark:text-stone-200">
  <div class="mx-auto flex max-w-5xl gap-8 px-6 py-8">
    <nav class="w-36 flex-none text-sm" aria-label={t('nav.main')}>
      <ul class="space-y-0.5">
        {#each links as link (link.name)}
          {@const active = current === link.name}
          <li>
            <a
              href={link.href}
              aria-current={active ? 'page' : undefined}
              class="block rounded-md px-2 py-1 transition-colors {active
                ? 'bg-stone-200/70 font-semibold dark:bg-stone-800'
                : 'text-stone-600 hover:bg-stone-100 dark:text-stone-400 dark:hover:bg-stone-900'}"
            >{t(link.label)}</a>
          </li>
        {/each}
      </ul>
      <button
        type="button"
        onclick={openPalette}
        class="mt-4 px-2 text-xs text-stone-400 hover:text-stone-600 dark:text-stone-500 dark:hover:text-stone-300"
      >
        <kbd class="font-sans">Ctrl+K</kbd> {t('nav.search')}
      </button>
    </nav>

    <main class="min-w-0 flex-1">
      {@render children()}
    </main>
  </div>

  <CommandPalette />
  <Toast />
</div>
