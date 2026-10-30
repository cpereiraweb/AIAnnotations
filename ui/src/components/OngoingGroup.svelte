<script lang="ts">
  import { copyText } from '../lib/actions';
  import { getLang, t } from '../lib/i18n';
  import { relativeTime } from '../lib/format';
  import { ongoingResumeCommand } from '../lib/shell';
  import type { OngoingGroup } from '../lib/types';

  let { group, now }: { group: OngoingGroup; now: number } = $props();
</script>

<div class="rounded-lg border border-stone-200 bg-white px-3 py-2.5 dark:border-stone-800 dark:bg-stone-900" data-ongoing-group={group.project_id}>
  <div class="flex items-center gap-2">
    <a href="#/projects/{group.project_id}" class="min-w-0 flex-1 truncate font-semibold hover:underline">
      {group.project_name}{#if group.items.length > 1}{" "}<span class="font-normal text-stone-400">· {group.items.length}</span>{/if}
    </a>
  </div>

  <ul class="mt-1.5 space-y-1.5">
    {#each group.items as o (o.file)}
      <li class="flex items-center gap-2">
        <span class="min-w-0 flex-1 truncate text-stone-600 dark:text-stone-300" title={o.file}>{o.title}</span>
        <span class="flex-none text-[11px] text-stone-400">{relativeTime(o.modified_ms, now, getLang())}</span>
        <button
          type="button"
          class="flex-none rounded border border-stone-300 px-2 py-px text-[11px] text-stone-600 transition-colors hover:bg-stone-100 hover:text-stone-900 dark:border-stone-700 dark:text-stone-400 dark:hover:bg-stone-800 dark:hover:text-stone-100"
          title={ongoingResumeCommand(o.project_path, o.name)}
          onclick={() => copyText(ongoingResumeCommand(o.project_path, o.name))}
        >{t('action.resume')}</button>
      </li>
    {/each}
  </ul>
</div>
