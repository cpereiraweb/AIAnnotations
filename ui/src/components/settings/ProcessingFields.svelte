<script lang="ts">
  import { t } from '../../lib/i18n';
  import type { ModelOption, OnboardingView, SettingsPatch, Tool } from '../../lib/types';

  let {
    view,
    provider,
    claudeModel,
    claudeBriefingModel,
    codexModel,
    codexBriefingModel,
    onchange,
  }: {
    view: OnboardingView;
    provider: Tool;
    claudeModel: string;
    claudeBriefingModel: string;
    codexModel: string;
    codexBriefingModel: string;
    onchange: (field: keyof SettingsPatch, value: string) => void;
  } = $props();

  const TOOL_NAMES: Record<Tool, string> = { claude: 'Claude Code', codex: 'Codex' };

  const detected = $derived(view.cards.filter((c) => c.bin !== null));

  /** The select's options; a saved value missing from the list stays selectable. */
  function modelOptions(models: ModelOption[], current: string, codexDefault: boolean): ModelOption[] {
    const options = codexDefault ? [{ value: '', label: t('onboarding.processing.codex_default') }] : [];
    options.push(...models);
    if (current && !options.some((o) => o.value === current)) options.push({ value: current, label: current });
    return options;
  }

  const input = 'w-full rounded border border-stone-300 bg-white px-2 py-1 dark:border-stone-700 dark:bg-stone-900';
  const box = 'rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900';
</script>

{#snippet modelSelect(field: keyof SettingsPatch, models: ModelOption[], current: string, codexDefault: boolean)}
  <select class={input} value={current} onchange={(e) => onchange(field, e.currentTarget.value)}>
    {#each modelOptions(models, current, codexDefault) as m (m.value)}
      <option value={m.value} selected={m.value === current}>{m.label}</option>
    {/each}
  </select>
{/snippet}

{#if detected.length === 0}
  <p class="text-amber-700 dark:text-amber-400">{t('onboarding.processing.none')}</p>
{:else}
  <fieldset class="space-y-1">
    <legend class="mb-1 text-xs text-stone-500 dark:text-stone-400">
      {t('onboarding.processing.provider')}
    </legend>
    <!-- Segmented control: a radio group, so screen readers announce the choice. -->
    <div class="inline-flex overflow-hidden rounded border border-stone-300 dark:border-stone-700">
      {#each view.cards as c (c.tool)}
        <label
          class="cursor-pointer px-3 py-1 text-xs transition-colors has-[:checked]:bg-stone-800 has-[:checked]:text-white has-[:disabled]:cursor-not-allowed has-[:disabled]:opacity-40 has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-stone-400 dark:has-[:checked]:bg-stone-200 dark:has-[:checked]:text-stone-900 [&+&]:border-l [&+&]:border-stone-300 dark:[&+&]:border-stone-700"
        >
          <input
            type="radio"
            name="provider"
            class="sr-only"
            value={c.tool}
            disabled={c.bin === null}
            checked={provider === c.tool}
            onchange={() => onchange('provider', c.tool)}
          />
          {TOOL_NAMES[c.tool]}
        </label>
      {/each}
    </div>
  </fieldset>
{/if}
<div class="grid gap-3 sm:grid-cols-2">
  {#each view.cards as c (c.tool)}
    {@const found = c.bin !== null}
    {@const chosen = found && provider === c.tool}
    {@const codex = c.tool === 'codex'}
    <fieldset
      class="{box} space-y-2 {chosen ? 'border-stone-500 ring-1 ring-stone-500 dark:border-stone-400 dark:ring-stone-400' : ''} {found ? '' : 'opacity-50'}"
      disabled={!found}
    >
      <legend class="sr-only">{TOOL_NAMES[c.tool]}</legend>
      <div class="flex items-center justify-between gap-2" aria-hidden="true">
        <span class="font-semibold">{TOOL_NAMES[c.tool]}</span>
        {#if chosen}
          <span class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.processing.chosen')}</span>
        {:else if !found}
          <span class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.processing.not_found')}</span>
        {/if}
      </div>
      <label class="block space-y-1">
        <span class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.processing.summary_model')}</span>
        {#if codex}
          {@render modelSelect('codex_model', view.codex_models, codexModel, true)}
        {:else}
          {@render modelSelect('claude_model', view.claude_models, claudeModel, false)}
        {/if}
      </label>
      <label class="block space-y-1">
        <span class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.processing.briefing_model')}</span>
        {#if codex}
          {@render modelSelect('codex_briefing_model', view.codex_models, codexBriefingModel, true)}
        {:else}
          {@render modelSelect('claude_briefing_model', view.claude_models, claudeBriefingModel, false)}
        {/if}
      </label>
    </fieldset>
  {/each}
</div>
