<script lang="ts">
  import { onMount } from 'svelte';
  import {
    describeError,
    finish_onboarding,
    import_history,
    onboarding_state,
    onImportDone,
    onImportProgress,
    write_hooks,
  } from '../lib/api';
  import { t } from '../lib/i18n';
  import { toast } from '../lib/toast.svelte';
  import type { Choices, CliCard, HistoryCounts, OnboardingView, Tool } from '../lib/types';

  let { onfinish }: { onfinish: () => void } = $props();

  const STEPS = ['detect', 'hooks', 'processing', 'history', 'autostart'] as const;
  const TOOL_NAMES: Record<Tool, string> = { claude: 'Claude Code', codex: 'Codex' };

  let view = $state<OnboardingView | null>(null);
  let error = $state<string | null>(null);
  let step = $state(0);

  // The choices, prefilled from the view.
  let provider = $state<Tool>('claude');
  let claudeModel = $state('');
  let claudeBriefingModel = $state('');
  let codexModel = $state('');
  let codexBriefingModel = $state('');
  let summarizeHistory = $state(false);
  let autostart = $state(true);

  let written = $state<Record<Tool, boolean>>({ claude: false, codex: false });
  let writing = $state<Tool | null>(null);
  let importing = $state(false);
  let progress = $state<{ done: number; total: number } | null>(null);
  let importCounts = $state<HistoryCounts | null>(null);
  let importError = $state<string | null>(null);
  let finishing = $state(false);

  async function load(prefill: boolean) {
    try {
      const next = await onboarding_state();
      view = next;
      error = null;
      if (prefill) {
        provider = next.provider;
        claudeModel = next.claude_model;
        claudeBriefingModel = next.claude_briefing_model;
        codexModel = next.codex_model ?? '';
        codexBriefingModel = next.codex_briefing_model ?? '';
        summarizeHistory = next.summarize_history;
        autostart = next.autostart;
      }
    } catch (e) {
      error = describeError(e);
    }
  }

  onMount(() => {
    void load(true);
    const stopProgress = onImportProgress((p) => {
      progress = p;
    });
    const stopDone = onImportDone((d) => {
      importing = false;
      progress = null;
      importCounts = d.counts;
      importError = d.error ? describeError(d.error) : null;
      // The estimate of the AI summaries depends on what was just imported.
      void load(false);
    });
    return () => {
      stopProgress();
      stopDone();
    };
  });

  const detected = $derived(view?.cards.filter((c) => c.bin !== null) ?? []);
  const toolName = (tool: Tool) => TOOL_NAMES[tool];
  const counts = $derived(importCounts ?? (view && view.history.sessions > 0 ? view.history : null));

  function hookNotice(card: CliCard, cliFound: boolean): string | null {
    if (card.invalid_json) return t('onboarding.hooks.invalid');
    if (!cliFound) return t('onboarding.hooks.no_cli');
    if (card.hook_status === 'current') return t('onboarding.hooks.current');
    return null;
  }

  async function writeHook(card: CliCard) {
    writing = card.tool;
    try {
      await write_hooks(card.tool);
      written[card.tool] = true;
      toast(t('toast.hook_written'));
      await load(false);
    } catch (e) {
      toast(describeError(e));
    } finally {
      writing = null;
    }
  }

  async function importAll() {
    importing = true;
    importError = null;
    importCounts = null;
    progress = { done: 0, total: 0 };
    try {
      await import_history();
    } catch (e) {
      importing = false;
      progress = null;
      toast(describeError(e));
    }
  }

  async function finish() {
    const choices: Choices = {
      provider,
      claude_model: claudeModel,
      claude_briefing_model: claudeBriefingModel,
      codex_model: codexModel,
      codex_briefing_model: codexBriefingModel,
      summarize_history: summarizeHistory,
      autostart,
    };
    finishing = true;
    try {
      const warning = await finish_onboarding(choices);
      if (warning) toast(t('toast.autostart_failed', { message: warning }));
      onfinish();
    } catch (e) {
      toast(describeError(e));
    } finally {
      finishing = false;
    }
  }

  const button =
    'rounded border border-stone-300 px-3 py-1 text-xs text-stone-700 transition-colors hover:bg-stone-100 disabled:cursor-not-allowed disabled:opacity-50 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800';
  const input =
    'w-full rounded border border-stone-300 bg-white px-2 py-1 dark:border-stone-700 dark:bg-stone-900';
  const card = 'rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900';
</script>

<div class="min-h-screen bg-stone-50 text-[13px] leading-normal text-stone-800 antialiased dark:bg-stone-950 dark:text-stone-200">
  <div class="mx-auto max-w-2xl space-y-5 px-6 py-8">
    <header>
      <h1 class="text-lg font-semibold">{t('onboarding.title')}</h1>
      <p class="text-xs text-stone-500 dark:text-stone-400">
        {t('onboarding.step', { n: step + 1, total: STEPS.length })}
      </p>
    </header>

    {#if error}
      <p class="text-red-600 dark:text-red-400">{error}</p>
    {:else if view}
      {@const v = view}
      {#if STEPS[step] === 'detect'}
        <section class="space-y-2">
          <h2 class="text-sm font-semibold">{t('onboarding.detect.title')}</h2>
          <ul class="space-y-2">
            {#each v.cards as c (c.tool)}
              <li class={card}>
                {#if c.bin}
                  {t('onboarding.detect.found', { tool: toolName(c.tool), bin: c.bin })}
                {:else}
                  <span class="text-stone-500 dark:text-stone-400">
                    {t('onboarding.detect.not_found', { tool: toolName(c.tool) })}
                  </span>
                {/if}
              </li>
            {/each}
          </ul>
        </section>
      {:else if STEPS[step] === 'hooks'}
        <section class="space-y-2">
          <h2 class="text-sm font-semibold">{t('onboarding.hooks.title')}</h2>
          {#if detected.length === 0}
            <p class="text-stone-500 dark:text-stone-400">{t('onboarding.hooks.none')}</p>
          {/if}
          {#each detected as c (c.tool)}
            {@const notice = hookNotice(c, v.cli_found)}
            <article class="{card} space-y-2">
              <h3 class="font-semibold">{toolName(c.tool)}</h3>
              <p class="text-xs break-all text-stone-500 dark:text-stone-400">
                {t('onboarding.hooks.path', { path: c.config_path })}
              </p>
              {#if c.diff}
                <pre
                  class="max-h-64 overflow-auto rounded bg-stone-100 p-2 text-[11px] dark:bg-stone-950">{c.diff}</pre>
              {/if}
              {#if notice}
                <p class="text-stone-600 dark:text-stone-400">{notice}</p>
              {/if}
              <button
                type="button"
                class={button}
                disabled={!c.can_write || c.invalid_json || !v.cli_found || c.hook_status === 'current' || writing !== null}
                onclick={() => writeHook(c)}
              >{t('onboarding.hooks.write')}</button>
              {#if c.tool === 'codex' && written.codex}
                <p class="text-amber-700 dark:text-amber-400">{t('onboarding.hooks.codex_trust')}</p>
              {/if}
            </article>
          {/each}
          <p class="text-xs text-stone-400">{t('onboarding.hooks.skip')}</p>
        </section>
      {:else if STEPS[step] === 'processing'}
        <section class="space-y-3">
          <h2 class="text-sm font-semibold">{t('onboarding.processing.title')}</h2>
          {#if detected.length === 0}
            <p class="text-amber-700 dark:text-amber-400">{t('onboarding.processing.none')}</p>
          {:else}
            <fieldset class="space-y-1">
              <legend class="mb-1 text-xs text-stone-500 dark:text-stone-400">
                {t('onboarding.processing.provider')}
              </legend>
              {#each detected as c (c.tool)}
                <label class="flex items-center gap-2">
                  <input type="radio" name="provider" value={c.tool} bind:group={provider} />
                  {toolName(c.tool)}
                </label>
              {/each}
            </fieldset>
          {/if}
          <div class="grid gap-3 sm:grid-cols-2">
            <label class="block space-y-1">
              <span class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.processing.summary_model')}</span>
              {#if provider === 'codex'}
                <input class={input} bind:value={codexModel} />
              {:else}
                <input class={input} bind:value={claudeModel} />
              {/if}
            </label>
            <label class="block space-y-1">
              <span class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.processing.briefing_model')}</span>
              {#if provider === 'codex'}
                <input class={input} bind:value={codexBriefingModel} />
              {:else}
                <input class={input} bind:value={claudeBriefingModel} />
              {/if}
            </label>
          </div>
        </section>
      {:else if STEPS[step] === 'history'}
        <section class="space-y-3">
          <h2 class="text-sm font-semibold">{t('onboarding.history.title')}</h2>
          <button type="button" class={button} disabled={importing} onclick={importAll}>
            {t('onboarding.history.import')}
          </button>
          {#if importing && progress}
            <div class="space-y-1">
              <progress
                class="w-full"
                value={progress.done}
                max={Math.max(progress.total, 1)}
                aria-label={t('onboarding.history.progress')}
              ></progress>
              <p class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.history.progress')}</p>
            </div>
          {/if}
          {#if importError}
            <p class="text-red-600 dark:text-red-400">{t('onboarding.history.error', { message: importError })}</p>
          {:else if counts && !importing}
            <p>{t('onboarding.history.counts', { ...counts })}</p>
          {/if}
          <div class="{card} space-y-1">
            <label class="flex items-center gap-2">
              <input type="checkbox" bind:checked={summarizeHistory} />
              {t('onboarding.history.summarize')}
            </label>
            <p class="text-xs text-stone-500 dark:text-stone-400">
              {t('onboarding.history.estimate', { cli: toolName(provider), count: v.pending_backfill })}
            </p>
          </div>
        </section>
      {:else}
        <section class="space-y-2">
          <h2 class="text-sm font-semibold">{t('onboarding.autostart.title')}</h2>
          <label class="flex items-center gap-2">
            <input type="checkbox" role="switch" bind:checked={autostart} />
            {t('onboarding.autostart.label')}
          </label>
        </section>
      {/if}

      <footer class="flex justify-between pt-2">
        <button type="button" class={button} disabled={step === 0} onclick={() => (step -= 1)}>
          {t('onboarding.back')}
        </button>
        {#if step < STEPS.length - 1}
          <button type="button" class={button} onclick={() => (step += 1)}>{t('onboarding.next')}</button>
        {:else}
          <button type="button" class={button} disabled={finishing} onclick={finish}>
            {t('onboarding.finish')}
          </button>
        {/if}
      </footer>
    {/if}
  </div>
</div>
