<script lang="ts">
  import { onMount } from 'svelte';
  import { describeError, finish_onboarding, onboarding_state } from '../lib/api';
  import { importState } from '../lib/importState.svelte';
  import { TOOL_NAMES } from '../lib/tools';
  import { box, button } from '../lib/ui-classes';
  import { t } from '../lib/i18n';
  import { toast } from '../lib/toast.svelte';
  import HookCard from '../components/settings/HookCard.svelte';
  import HistoryImport from '../components/settings/HistoryImport.svelte';
  import ProcessingFields from '../components/settings/ProcessingFields.svelte';
  import type { Choices, OnboardingView, SettingsPatch, Tool } from '../lib/types';

  let { onfinish }: { onfinish: () => void } = $props();

  const STEPS = ['detect', 'hooks', 'processing', 'history', 'autostart'] as const;

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
  });

  // The estimate of the AI summaries depends on what was just imported.
  let seenImports = importState.doneCount;
  $effect(() => {
    const done = importState.doneCount;
    if (done === seenImports) return;
    seenImports = done;
    void load(false);
  });

  const v_done = $derived(view?.done ?? false);
  const detected = $derived(view?.cards.filter((c) => c.bin !== null) ?? []);
  const toolName = (tool: Tool) => TOOL_NAMES[tool];
  /** Wizard mode: choices are only kept locally and sent on "Concluir". */
  function setChoice(field: keyof SettingsPatch, value: string) {
    if (field === 'provider') provider = value as Tool;
    else if (field === 'claude_model') claudeModel = value;
    else if (field === 'claude_briefing_model') claudeBriefingModel = value;
    else if (field === 'codex_model') codexModel = value;
    else if (field === 'codex_briefing_model') codexBriefingModel = value;
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

</script>

<div class="min-h-screen bg-stone-50 text-[13px] leading-normal text-stone-800 antialiased dark:bg-stone-950 dark:text-stone-200">
  <div class="mx-auto max-w-2xl space-y-5 px-6 py-8">
    <header>
      {#if v_done}
        <a href="#/" class="text-xs text-stone-500 underline dark:text-stone-400">{t('onboarding.to_today')}</a>
      {/if}
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
              <li class={box}>
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
            <HookCard card={c} cliFound={v.cli_found} onchange={() => load(false)} />
          {/each}
          <p class="text-xs text-stone-400">{t('onboarding.hooks.skip')}</p>
        </section>
      {:else if STEPS[step] === 'processing'}
        <section class="space-y-3">
          <h2 class="text-sm font-semibold">{t('onboarding.processing.title')}</h2>
          <ProcessingFields
            view={v}
            {provider}
            {claudeModel}
            {claudeBriefingModel}
            {codexModel}
            {codexBriefingModel}
            onchange={setChoice}
          />
        </section>
      {:else if STEPS[step] === 'history'}
        <section class="space-y-3">
          <h2 class="text-sm font-semibold">{t('onboarding.history.title')}</h2>
          <HistoryImport view={v} {summarizeHistory} {provider} onsummarize={(on) => (summarizeHistory = on)} />
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
