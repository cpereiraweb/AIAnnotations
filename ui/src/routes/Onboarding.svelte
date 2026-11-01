<script lang="ts">
  import { onMount } from 'svelte';
  import {
    describeError,
    describeErrorBare,
    finish_onboarding,
    remove_hooks,
    import_history,
    onboarding_state,
    onImportDone,
    onImportProgress,
    write_hooks,
  } from '../lib/api';
  import { t } from '../lib/i18n';
  import { toast } from '../lib/toast.svelte';
  import type { Choices, CliCard, HistoryCounts, ModelOption, OnboardingView, Tool } from '../lib/types';

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
  // Off by default: the Laravel app's hooks are only replaced when the user asks.
  let replaceLegacy = $state<Record<Tool, boolean>>({ claude: false, codex: false });
  let writing = $state<Tool | null>(null);
  // The card whose removal diff is on screen, waiting for "Confirmar remoção".
  let confirmingRemove = $state<Tool | null>(null);
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
      // A failed refresh keeps the previous view; only the first load replaces the wizard.
      if (view) toast(describeError(e));
      else error = describeError(e);
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
      importError = d.error ? describeErrorBare(d.error) : null;
      // The estimate of the AI summaries depends on what was just imported.
      void load(false);
    });
    return () => {
      stopProgress();
      stopDone();
    };
  });

  const v_done = $derived(view?.done ?? false);
  const detected = $derived(view?.cards.filter((c) => c.bin !== null) ?? []);
  const toolName = (tool: Tool) => TOOL_NAMES[tool];
  /** The select's options; a saved value missing from the list stays selectable. */
  function modelOptions(models: ModelOption[], current: string, codexDefault: boolean): ModelOption[] {
    const options = codexDefault ? [{ value: '', label: t('onboarding.processing.codex_default') }] : [];
    options.push(...models);
    if (current && !options.some((o) => o.value === current)) options.push({ value: current, label: current });
    return options;
  }

  const counts = $derived(importCounts ?? (view && view.history.sessions > 0 ? view.history : null));

  /** The diff of what "Gravar hook" writes, given the "replace the Laravel hooks" choice. */
  function shownDiff(card: CliCard): string | null {
    return card.legacy && replaceLegacy[card.tool] ? card.diff_replace : card.diff;
  }

  function hookNotice(card: CliCard, cliFound: boolean): string | null {
    if (card.invalid_json) return t('onboarding.hooks.invalid');
    if (!cliFound) return t('onboarding.hooks.no_cli');
    if (card.hook_status === 'current' && !shownDiff(card)) return t('onboarding.hooks.current');
    return null;
  }

  async function writeHook(card: CliCard) {
    // Defense in depth: never write without the diff on screen.
    if (!shownDiff(card)) return;
    writing = card.tool;
    try {
      await write_hooks(card.tool, card.legacy && replaceLegacy[card.tool]);
      written[card.tool] = true;
      toast(t('toast.hook_written'));
      await load(false);
    } catch (e) {
      toast(describeError(e));
    } finally {
      writing = null;
    }
  }

  async function removeHook(card: CliCard) {
    // Same rule as writing: never touch the file without the diff on screen.
    if (!card.diff_remove || confirmingRemove !== card.tool) return;
    writing = card.tool;
    try {
      await remove_hooks(card.tool);
      written[card.tool] = false;
      confirmingRemove = null;
      toast(t('toast.hook_removed'));
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
      else if (v_done) toast(t('toast.settings_saved'));
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
      {#if v_done}
        <a href="#/" class="text-xs text-stone-500 underline dark:text-stone-400">{t('onboarding.to_today')}</a>
      {/if}
      <h1 class="text-lg font-semibold">{t(v_done ? 'onboarding.settings_title' : 'onboarding.title')}</h1>
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
            {@const diff = shownDiff(c)}
            <article class="{card} space-y-2">
              <h3 class="font-semibold">{toolName(c.tool)}</h3>
              <p class="text-xs break-all text-stone-500 dark:text-stone-400">
                {t('onboarding.hooks.path', { path: c.config_path })}
              </p>
              {#if c.legacy && c.can_write && v.cli_found}
                <div class="space-y-1">
                  <p class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.hooks.legacy_hint')}</p>
                  <label class="flex items-center gap-2">
                    <input type="checkbox" bind:checked={replaceLegacy[c.tool]} disabled={writing !== null} />
                    {t('onboarding.hooks.replace_legacy')}
                  </label>
                </div>
              {/if}
              {#if confirmingRemove === c.tool && c.diff_remove}
                <p class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.hooks.remove_hint')}</p>
                <pre
                  class="max-h-64 overflow-auto rounded bg-stone-100 p-2 text-[11px] dark:bg-stone-950">{c.diff_remove}</pre>
                <div class="flex gap-2">
                  <button type="button" class={button} disabled={writing !== null} onclick={() => removeHook(c)}
                    >{t('onboarding.hooks.remove_confirm')}</button>
                  <button type="button" class={button} disabled={writing !== null} onclick={() => (confirmingRemove = null)}
                    >{t('onboarding.hooks.remove_cancel')}</button>
                </div>
              {:else}
                {#if diff}
                  <pre
                    class="max-h-64 overflow-auto rounded bg-stone-100 p-2 text-[11px] dark:bg-stone-950">{diff}</pre>
                {/if}
                {#if notice}
                  <p class="text-stone-600 dark:text-stone-400">{notice}</p>
                {/if}
                <div class="flex gap-2">
                  <button
                    type="button"
                    class={button}
                    disabled={!c.can_write || !diff || c.invalid_json || !v.cli_found || writing !== null}
                    onclick={() => writeHook(c)}
                  >{t('onboarding.hooks.write')}</button>
                  {#if c.diff_remove}
                    <button type="button" class={button} disabled={writing !== null} onclick={() => (confirmingRemove = c.tool)}
                      >{t('onboarding.hooks.remove')}</button>
                  {/if}
                </div>
              {/if}
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
              <!-- Segmented control: a radio group, so screen readers announce the choice. -->
              <div class="inline-flex overflow-hidden rounded border border-stone-300 dark:border-stone-700">
                {#each v.cards as c (c.tool)}
                  <label
                    class="cursor-pointer px-3 py-1 text-xs transition-colors has-[:checked]:bg-stone-800 has-[:checked]:text-white has-[:disabled]:cursor-not-allowed has-[:disabled]:opacity-40 has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-stone-400 dark:has-[:checked]:bg-stone-200 dark:has-[:checked]:text-stone-900 [&+&]:border-l [&+&]:border-stone-300 dark:[&+&]:border-stone-700"
                  >
                    <input
                      type="radio"
                      name="provider"
                      class="sr-only"
                      value={c.tool}
                      disabled={c.bin === null}
                      bind:group={provider}
                    />
                    {toolName(c.tool)}
                  </label>
                {/each}
              </div>
            </fieldset>
          {/if}
          <div class="grid gap-3 sm:grid-cols-2">
            {#each v.cards as c (c.tool)}
              {@const found = c.bin !== null}
              {@const chosen = found && provider === c.tool}
              {@const codex = c.tool === 'codex'}
              <fieldset
                class="{card} space-y-2 {chosen ? 'border-stone-500 ring-1 ring-stone-500 dark:border-stone-400 dark:ring-stone-400' : ''} {found ? '' : 'opacity-50'}"
                disabled={!found}
              >
                <legend class="sr-only">{toolName(c.tool)}</legend>
                <div class="flex items-center justify-between gap-2" aria-hidden="true">
                  <span class="font-semibold">{toolName(c.tool)}</span>
                  {#if chosen}
                    <span class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.processing.chosen')}</span>
                  {:else if !found}
                    <span class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.processing.not_found')}</span>
                  {/if}
                </div>
                <label class="block space-y-1">
                  <span class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.processing.summary_model')}</span>
                  {#if codex}
                    <select class={input} bind:value={codexModel}>
                      {#each modelOptions(v.codex_models, codexModel, true) as m (m.value)}
                        <option value={m.value}>{m.label}</option>
                      {/each}
                    </select>
                  {:else}
                    <select class={input} bind:value={claudeModel}>
                      {#each modelOptions(v.claude_models, claudeModel, false) as m (m.value)}
                        <option value={m.value}>{m.label}</option>
                      {/each}
                    </select>
                  {/if}
                </label>
                <label class="block space-y-1">
                  <span class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.processing.briefing_model')}</span>
                  {#if codex}
                    <select class={input} bind:value={codexBriefingModel}>
                      {#each modelOptions(v.codex_models, codexBriefingModel, true) as m (m.value)}
                        <option value={m.value}>{m.label}</option>
                      {/each}
                    </select>
                  {:else}
                    <select class={input} bind:value={claudeBriefingModel}>
                      {#each modelOptions(v.claude_models, claudeBriefingModel, false) as m (m.value)}
                        <option value={m.value}>{m.label}</option>
                      {/each}
                    </select>
                  {/if}
                </label>
              </fieldset>
            {/each}
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
            {t(v_done ? 'onboarding.save' : 'onboarding.finish')}
          </button>
        {/if}
      </footer>
    {/if}
  </div>
</div>
