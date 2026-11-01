<script lang="ts">
  import { onMount } from 'svelte';
  import BriefingCard from '../components/BriefingCard.svelte';
  import Disclosure from '../components/Disclosure.svelte';
  import InboxGroupCard from '../components/InboxGroup.svelte';
  import OngoingGroupCard from '../components/OngoingGroup.svelte';
  import {
    describeError,
    ERR_BRIEFING_RUNNING,
    dismiss,
    dismiss_group,
    import_history,
    onChanged,
    regenerate_briefing,
    today,
  } from '../lib/api';
  import { t } from '../lib/i18n';
  import { toast } from '../lib/toast.svelte';
  import type { InboxGroup, InboxItem, TodayView } from '../lib/types';

  const REFRESH_MS = 30_000;
  const ONGOING_STALE_DAYS = 30;

  let view = $state<TodayView | null>(null);
  let error = $state<string | null>(null);
  let now = $state(Date.now());
  // "regerar" was clicked and no reload has confirmed the state yet.
  let regenerating = $state(false);

  async function load() {
    try {
      const next = await today();
      view = next;
      error = null;
      regenerating = false;
    } catch (e) {
      error = describeError(e);
    } finally {
      now = Date.now();
    }
  }

  onMount(() => {
    void load();
    const stop = onChanged(() => void load());
    // Relative times and the git state refresh even when nothing was written.
    const timer = setInterval(() => void load(), REFRESH_MS);
    return () => {
      stop();
      clearInterval(timer);
    };
  });

  const count = (groups: InboxGroup[]) => groups.reduce((n, g) => n + g.items.length, 0);
  const localDate = (ms: number) => {
    const d = new Date(ms);
    const pad = (n: number) => String(n).padStart(2, '0');
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  };

  /** Runs a command; resolves to whether it succeeded (failures become a toast). */
  async function run(action: () => Promise<unknown>): Promise<boolean> {
    try {
      await action();
      return true;
    } catch (e) {
      toast(describeError(e));
      return false;
    }
  }

  // The dismiss commands do not emit "changed", so the screen reloads itself after them.
  async function dismissThenReload(action: () => Promise<unknown>) {
    if (await run(action)) await load();
  }

  const dismissItem = (item: InboxItem) => dismissThenReload(() => dismiss(item.key, item.state_hash));
  const dismissAll = (group: InboxGroup, older: boolean) =>
    dismissThenReload(() => dismiss_group(group.project_id, older));

  async function regenerate() {
    regenerating = true;
    try {
      await regenerate_briefing();
    } catch (e) {
      toast(describeError(e));
      // Already running: the generation that holds the lock will emit "changed" and clear the
      // state. Any other failure means nothing is generating, so go back now.
      if (e !== ERR_BRIEFING_RUNNING) regenerating = false;
    }
  }

  async function importHistory() {
    toast(t('toast.import_started'));
    await run(import_history);
  }
</script>

{#if error}
  <p class="text-red-600 dark:text-red-400">{error}</p>
{:else if view}
  {@const v = view}
  {@const ai = v.ai_attention}
  <div class="space-y-7">
    {#if v.hooks_need_update}
      <a
        href="#/settings"
        class="block rounded-lg border border-amber-300 bg-amber-50 px-4 py-2 text-amber-900 hover:underline dark:border-amber-700 dark:bg-amber-950 dark:text-amber-200"
      >{t('today.hooks_outdated')}</a>
    {/if}

    {#if ai && ai.kind !== 'briefing_failed'}
      <p
        class="rounded-lg border border-amber-300 bg-amber-50 px-4 py-2 break-words text-amber-900 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-200"
      >{ai.kind === 'login'
        ? t('today.ai_login', { tool: ai.tool === 'codex' ? 'Codex' : 'Claude Code', bin: ai.tool })
        : t('today.ai_unavailable', { detail: ai.detail })}</p>
    {/if}

    <BriefingCard
      briefing={v.briefing}
      generating={regenerating || v.briefing_running}
      paused={v.summaries_paused}
      startFailed={v.briefing_start_failed}
      failedDetail={ai?.kind === 'briefing_failed' ? ai.detail : null}
      today={localDate(now)}
      onregenerate={regenerate}
    />

    {#if v.is_empty_db}
      <section class="rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900">
        <p class="text-stone-500 dark:text-stone-400">{t('today.empty_db')}</p>
        <button
          type="button"
          onclick={importHistory}
          class="mt-2 rounded border border-stone-300 px-2 py-0.5 text-xs text-stone-700 transition-colors hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800"
        >{t('today.import')}</button>
      </section>
    {/if}

    {#if v.ongoings_recent.length > 0 || v.ongoings_stale.length > 0}
      <section>
        <h2 class="mb-2 text-xs font-semibold tracking-wide text-stone-500 uppercase dark:text-stone-400">
          {t('ongoing.title', { count: v.ongoings_recent.reduce((n, g) => n + g.items.length, 0) })}
        </h2>
        {#if v.ongoings_recent.length === 0}
          <p class="text-stone-400">{t('ongoing.none_recent', { days: ONGOING_STALE_DAYS })}</p>
        {:else}
          <div class="grid gap-2 sm:grid-cols-2">
            {#each v.ongoings_recent as group (group.project_id)}
              <OngoingGroupCard {group} {now} />
            {/each}
          </div>
        {/if}
        {#if v.ongoings_stale.length > 0}
          <Disclosure
            title={t('ongoing.stale', {
              days: ONGOING_STALE_DAYS,
              count: v.ongoings_stale.reduce((n, g) => n + g.items.length, 0),
            })}
          >
            <div class="grid gap-2 sm:grid-cols-2">
              {#each v.ongoings_stale as group (group.project_id)}
                <OngoingGroupCard {group} {now} />
              {/each}
            </div>
          </Disclosure>
        {/if}
      </section>
    {/if}

    <section>
      <h2 class="mb-2 text-xs font-semibold tracking-wide text-stone-500 uppercase dark:text-stone-400">
        {t('inbox.title', { count: count(v.inbox.recent) })}
      </h2>
      {#if v.inbox.recent.length === 0}
        <p class="text-stone-400">{t('inbox.empty')}</p>
      {:else}
        <div class="space-y-2">
          {#each v.inbox.recent as group (group.project_id)}
            <InboxGroupCard
              {group}
              runs={v.inbox_last_runs}
              ondismiss={dismissItem}
              ondismissall={(g) => dismissAll(g, false)}
            />
          {/each}
        </div>
      {/if}
      {#if v.inbox.older.length > 0}
        <Disclosure title={t('inbox.older', { count: count(v.inbox.older) })}>
          <div class="space-y-2">
            {#each v.inbox.older as group (group.project_id)}
              <InboxGroupCard
                {group}
                runs={v.inbox_last_runs}
                ondismiss={dismissItem}
                ondismissall={(g) => dismissAll(g, true)}
              />
            {/each}
          </div>
        </Disclosure>
      {/if}
    </section>

    {#if v.next_steps.length > 0}
      <section>
        <h2 class="mb-2 text-xs font-semibold tracking-wide text-stone-500 uppercase dark:text-stone-400">
          {t('today.next_steps')}
        </h2>
        <ul class="space-y-2">
          {#each v.next_steps as step (step.project_id + step.passo)}
            <li class="rounded-lg border border-stone-200 bg-white px-3 py-2 dark:border-stone-800 dark:bg-stone-900">
              {#if step.project_id}
                <a href="#/projects/{step.project_id}" class="font-semibold hover:underline">{step.projeto}</a>
              {:else}
                <span class="font-semibold">{step.projeto}</span>
              {/if}
              <span class="text-stone-400">·</span>
              {step.passo}
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if v.backfill}
      <footer class="pt-2 text-[11px] text-stone-400">
        {t('today.backfill', { done: v.backfill[0], total: v.backfill[1] })}
      </footer>
    {/if}
  </div>
{/if}
