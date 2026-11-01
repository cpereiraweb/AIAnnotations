<script lang="ts">
  import { onMount } from 'svelte';
  import {
    describeError,
    onboarding_state,
    save_settings,
    set_summaries_paused,
    today,
    worker_status,
  } from '../lib/api';
  import { importState } from '../lib/importState.svelte';
  import { setLang, t } from '../lib/i18n';
  import { toast } from '../lib/toast.svelte';
  import { box, button, input } from '../lib/ui-classes';
  import HookCard from '../components/settings/HookCard.svelte';
  import HistoryImport from '../components/settings/HistoryImport.svelte';
  import ProcessingFields from '../components/settings/ProcessingFields.svelte';
  import type { OnboardingView, SettingsPatch, Tool } from '../lib/types';

  let view = $state<OnboardingView | null>(null);
  let error = $state<string | null>(null);
  let backfill = $state<[number, number] | null>(null);
  let paused = $state(false);

  // Local copies of the controls, so a change shows at once and an error can put the old value back.
  let provider = $state<Tool>('claude');
  let claudeModel = $state('');
  let claudeBriefingModel = $state('');
  let codexModel = $state('');
  let codexBriefingModel = $state('');
  let summarizeHistory = $state(false);
  let autostart = $state(true);
  let langChoice = $state('');

  // Per field, the number of the latest change: only that change may roll the screen back.
  const latest: Record<string, number> = {};
  let counter = 0;

  function fill(v: OnboardingView) {
    view = v;
    provider = v.provider;
    claudeModel = v.claude_model;
    claudeBriefingModel = v.claude_briefing_model;
    codexModel = v.codex_model ?? '';
    codexBriefingModel = v.codex_briefing_model ?? '';
    summarizeHistory = v.summarize_history;
    autostart = v.autostart;
    langChoice = v.lang_choice ?? '';
  }

  async function load() {
    try {
      fill(await onboarding_state());
      error = null;
    } catch (e) {
      if (view) toast(describeError(e));
      else error = describeError(e);
    }
    // The pending-backfill line and the pause box are secondary: failing to read them is not fatal.
    today().then((v) => (backfill = v.backfill), () => {});
  }

  onMount(() => {
    void load();
    worker_status().then((s) => (paused = s.summarizing_paused_by_user), () => {});
  });

  // The estimate of the AI summaries depends on what was just imported.
  let seenImports = importState.doneCount;
  $effect(() => {
    const done = importState.doneCount;
    if (done === seenImports) return;
    seenImports = done;
    void load();
  });

  /** Saves one field at once. On error, the real values come back from the backend. */
  async function save(patch: Partial<SettingsPatch>) {
    const field = Object.keys(patch)[0];
    const mine = ++counter;
    latest[field] = mine;
    try {
      const warning = await save_settings(patch);
      if (patch.lang !== undefined) {
        // The new language applies once the view says which one is in effect.
        const v = await onboarding_state();
        if (latest[field] === mine) {
          fill(v);
          setLang(v.lang);
          document.documentElement.lang = v.lang;
        }
      }
      toast(warning ? t('toast.autostart_failed', { message: warning }) : t('toast.settings_saved'));
    } catch (e) {
      if (latest[field] === mine) await load();
      toast(describeError(e));
    }
  }

  function setChoice(field: keyof SettingsPatch, value: string) {
    if (field === 'provider') provider = value as Tool;
    else if (field === 'claude_model') claudeModel = value;
    else if (field === 'claude_briefing_model') claudeBriefingModel = value;
    else if (field === 'codex_model') codexModel = value;
    else if (field === 'codex_briefing_model') codexBriefingModel = value;
    void save({ [field]: value });
  }

  function setSummarize(on: boolean) {
    summarizeHistory = on;
    void save({ summarize_history: on });
  }

  function setAutostart(on: boolean) {
    autostart = on;
    void save({ autostart: on });
  }

  function setLanguage(value: string) {
    langChoice = value;
    void save({ lang: value === '' ? null : (value as 'pt-BR' | 'en') });
  }

  async function setPaused(on: boolean) {
    paused = on;
    try {
      paused = await set_summaries_paused(on);
      toast(t('toast.settings_saved'));
    } catch (e) {
      paused = !on;
      toast(describeError(e));
    }
  }

  const detected = $derived(view?.cards.filter((c) => c.bin !== null) ?? []);
  const h2 = 'text-sm font-semibold';
</script>

<div class="space-y-7">
  <h1 class="text-lg font-semibold">{t('settings.title')}</h1>

  {#if error}
    <p class="text-red-600 dark:text-red-400">{error}</p>
  {:else if view}
    {@const v = view}
    <section class="space-y-3">
      <h2 class={h2}>{t('settings.processing')}</h2>
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

    <section class="space-y-2">
      <h2 class={h2}>{t('settings.hooks')}</h2>
      {#if detected.length === 0}
        <p class="text-stone-500 dark:text-stone-400">{t('onboarding.hooks.none')}</p>
      {/if}
      {#each detected as c (c.tool)}
        <HookCard card={c} cliFound={v.cli_found} onchange={load} />
      {/each}
    </section>

    <section class="space-y-3">
      <h2 class={h2}>{t('settings.history')}</h2>
      <HistoryImport view={v} {summarizeHistory} {provider} onsummarize={setSummarize} />
      {#if backfill}
        <p class="text-xs text-stone-500 dark:text-stone-400">
          {t('settings.history_progress', { done: backfill[0], total: backfill[1] })}
        </p>
      {/if}
    </section>

    <section class="space-y-3">
      <h2 class={h2}>{t('settings.general')}</h2>
      <label class="block space-y-1">
        <span class="text-xs text-stone-500 dark:text-stone-400">{t('settings.lang')}</span>
        <select class={input} value={langChoice} onchange={(e) => setLanguage(e.currentTarget.value)}>
          <option value="" selected={langChoice === ''}>{t('settings.lang_auto')}</option>
          <option value="pt-BR" selected={langChoice === 'pt-BR'}>Português (Brasil)</option>
          <option value="en" selected={langChoice === 'en'}>English</option>
        </select>
      </label>
      <div class="{box} space-y-2">
        <label class="flex items-center gap-2">
          <input type="checkbox" checked={autostart} onchange={(e) => setAutostart(e.currentTarget.checked)} />
          {t('settings.autostart')}
        </label>
        <label class="flex items-center gap-2">
          <input type="checkbox" checked={paused} onchange={(e) => void setPaused(e.currentTarget.checked)} />
          {t('settings.pause')}
        </label>
      </div>
    </section>

    <section class="space-y-2">
      <h2 class={h2}>{t('settings.advanced')}</h2>
      <button type="button" class={button} onclick={() => (window.location.hash = '#/onboarding')}
        >{t('settings.redo_onboarding')}</button>
    </section>
  {/if}
</div>
