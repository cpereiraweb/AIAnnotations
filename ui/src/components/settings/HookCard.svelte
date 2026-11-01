<script lang="ts">
  import { describeError, remove_hooks, write_hooks } from '../../lib/api';
  import { t } from '../../lib/i18n';
  import { toast } from '../../lib/toast.svelte';
  import type { CliCard, Tool } from '../../lib/types';

  let { card, cliFound, onchange }: { card: CliCard; cliFound: boolean; onchange: () => Promise<void> } = $props();

  const TOOL_NAMES: Record<Tool, string> = { claude: 'Claude Code', codex: 'Codex' };

  let written = $state(false);
  // Off by default: the Laravel app's hooks are only replaced when the user asks.
  let replaceLegacy = $state(false);
  let writing = $state(false);
  // True while the removal diff is on screen, waiting for "Confirmar remoção".
  let confirmingRemove = $state(false);

  /** The diff of what "Gravar hook" writes, given the "replace the Laravel hooks" choice. */
  const diff = $derived(card.legacy && replaceLegacy ? card.diff_replace : card.diff);

  const notice = $derived.by(() => {
    if (card.invalid_json) return t('onboarding.hooks.invalid');
    if (!cliFound) return t('onboarding.hooks.no_cli');
    if (card.hook_status === 'current' && !diff) return t('onboarding.hooks.current');
    return null;
  });

  async function writeHook() {
    // Defense in depth: never write without the diff on screen.
    if (!diff) return;
    writing = true;
    try {
      await write_hooks(card.tool, card.legacy && replaceLegacy);
      written = true;
      toast(t('toast.hook_written'));
      await onchange();
    } catch (e) {
      toast(describeError(e));
    } finally {
      writing = false;
    }
  }

  async function removeHook() {
    // Same rule as writing: never touch the file without the diff on screen.
    if (!card.diff_remove || !confirmingRemove) return;
    writing = true;
    try {
      await remove_hooks(card.tool);
      written = false;
      confirmingRemove = false;
      toast(t('toast.hook_removed'));
      await onchange();
    } catch (e) {
      toast(describeError(e));
    } finally {
      writing = false;
    }
  }

  const button =
    'rounded border border-stone-300 px-3 py-1 text-xs text-stone-700 transition-colors hover:bg-stone-100 disabled:cursor-not-allowed disabled:opacity-50 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800';
  const box = 'rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900';
</script>

<article class="{box} space-y-2">
  <h3 class="font-semibold">{TOOL_NAMES[card.tool]}</h3>
  <p class="text-xs break-all text-stone-500 dark:text-stone-400">
    {t('onboarding.hooks.path', { path: card.config_path })}
  </p>
  {#if card.legacy && card.can_write && cliFound}
    <div class="space-y-1">
      <p class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.hooks.legacy_hint')}</p>
      <label class="flex items-center gap-2">
        <input type="checkbox" bind:checked={replaceLegacy} disabled={writing} />
        {t('onboarding.hooks.replace_legacy')}
      </label>
    </div>
  {/if}
  {#if confirmingRemove && card.diff_remove}
    <p class="text-xs text-stone-500 dark:text-stone-400">{t('onboarding.hooks.remove_hint')}</p>
    <pre
      class="max-h-64 overflow-auto rounded bg-stone-100 p-2 text-[11px] dark:bg-stone-950">{card.diff_remove}</pre>
    <div class="flex gap-2">
      <button type="button" class={button} disabled={writing} onclick={removeHook}
        >{t('onboarding.hooks.remove_confirm')}</button>
      <button type="button" class={button} disabled={writing} onclick={() => (confirmingRemove = false)}
        >{t('onboarding.hooks.remove_cancel')}</button>
    </div>
  {:else}
    {#if diff}
      <pre class="max-h-64 overflow-auto rounded bg-stone-100 p-2 text-[11px] dark:bg-stone-950">{diff}</pre>
    {/if}
    {#if notice}
      <p class="text-stone-600 dark:text-stone-400">{notice}</p>
    {/if}
    <div class="flex gap-2">
      <button
        type="button"
        class={button}
        disabled={!card.can_write || !diff || card.invalid_json || !cliFound || writing}
        onclick={writeHook}
      >{t('onboarding.hooks.write')}</button>
      {#if card.diff_remove}
        <button type="button" class={button} disabled={writing} onclick={() => (confirmingRemove = true)}
          >{t('onboarding.hooks.remove')}</button>
      {/if}
    </div>
  {/if}
  {#if card.tool === 'codex' && written}
    <p class="text-amber-700 dark:text-amber-400">{t('onboarding.hooks.codex_trust')}</p>
  {/if}
</article>
