<script lang="ts">
  import { t } from '../../lib/i18n';
  import { box, button, input } from '../../lib/ui-classes';
  import type { AppEntry, AppKind, AppsView, CustomApp, SettingsPatch } from '../../lib/types';

  let {
    view,
    onchange,
    onredetect,
  }: {
    view: AppsView;
    onchange: (patch: Partial<SettingsPatch>) => void;
    onredetect: () => void;
  } = $props();

  const hint = 'text-xs text-stone-500 dark:text-stone-400';

  /** The entries, plus the saved choice when it names something no longer detected. */
  function withChoice(list: AppEntry[], choice: string | null): AppEntry[] {
    if (choice && !list.some((a) => a.id === choice)) return [...list, { id: choice, label: choice, custom: false }];
    return list;
  }

  const editors = $derived(withChoice(view.editors, view.editor_choice));
  const terminals = $derived(withChoice(view.terminals, view.terminal_choice));
  const shells = $derived(
    view.shell_choice && !view.shells.includes(view.shell_choice) ? [...view.shells, view.shell_choice] : view.shells,
  );
  const editorName = $derived(view.editors.find((a) => a.id === view.editor)?.label ?? view.editor);
  const terminalName = $derived(view.terminals.find((a) => a.id === view.terminal)?.label ?? view.terminal ?? '');

  // The "Adicionar" form.
  let kind = $state<AppKind>('editor');
  let label = $state('');
  let exe = $state('');
  let args = $state('');
  const canAdd = $derived(label.trim() !== '' && exe.trim() !== '');

  const orNull = (value: string) => (value === '' ? null : value);

  function add() {
    if (!canAdd) return;
    const app: CustomApp = { id: '', label: label.trim(), kind, exe: exe.trim(), args: args.trim() };
    onchange({ custom_apps: [...view.custom_apps, app] });
    label = '';
    exe = '';
    args = '';
  }

  function remove(id: string) {
    onchange({ custom_apps: view.custom_apps.filter((a) => a.id !== id) });
  }

  function saveWorkspaces(value: string) {
    onchange({ workspaces_dir: value.trim() === '' ? null : value.trim() });
  }
</script>

<div class="grid gap-3 sm:grid-cols-3">
  <label class="block space-y-1">
    <span class={hint}>{t('settings.launch_editor')}</span>
    <select class={input} value={view.editor_choice ?? ''} onchange={(e) => onchange({ editor: orNull(e.currentTarget.value) })}>
      <option value="" selected={view.editor_choice === null}>{t('settings.launch_auto', { name: editorName })}</option>
      {#each editors as a (a.id)}
        <option value={a.id} selected={a.id === view.editor_choice}>{a.label}</option>
      {/each}
    </select>
  </label>
  <label class="block space-y-1">
    <span class={hint}>{t('settings.launch_terminal')}</span>
    {#if terminals.length === 0}
      <select class={input} disabled>
        <option>{t('settings.launch_no_terminal')}</option>
      </select>
    {:else}
      <select class={input} value={view.terminal_choice ?? ''} onchange={(e) => onchange({ terminal: orNull(e.currentTarget.value) })}>
        <option value="" selected={view.terminal_choice === null}>{t('settings.launch_auto', { name: terminalName })}</option>
        {#each terminals as a (a.id)}
          <option value={a.id} selected={a.id === view.terminal_choice}>{a.label}</option>
        {/each}
      </select>
    {/if}
  </label>
  <label class="block space-y-1">
    <span class={hint}>{t('settings.launch_shell')}</span>
    <select class={input} value={view.shell_choice ?? ''} onchange={(e) => onchange({ shell: orNull(e.currentTarget.value) })}>
      <option value="" selected={view.shell_choice === null}>{t('settings.launch_auto', { name: view.shell })}</option>
      {#each shells as p (p)}
        <option value={p} selected={p === view.shell_choice}>{p}</option>
      {/each}
    </select>
  </label>
</div>

<div class="space-y-1">
  <label for="workspaces-dir" class="block {hint}">{t('settings.launch_workspaces')}</label>
  <input
    id="workspaces-dir"
    type="text"
    class={input}
    aria-describedby="workspaces-dir-hint"
    value={view.workspaces_dir ?? ''}
    onchange={(e) => saveWorkspaces(e.currentTarget.value)}
  />
  <p id="workspaces-dir-hint" class={hint}>{t('settings.launch_workspaces_hint')}</p>
</div>

<div>
  <button type="button" class={button} onclick={onredetect}>{t('settings.launch_redetect')}</button>
</div>

<div class="{box} space-y-3">
  <h3 class="text-xs font-semibold">{t('settings.launch_custom')}</h3>
  {#if view.custom_apps.length === 0}
    <p class={hint}>{t('settings.launch_custom_none')}</p>
  {:else}
    <ul class="space-y-1">
      <!-- Unkeyed: a new app's id changes when the backend assigns it, and the row should stay put. -->
      {#each view.custom_apps as a}
        <li class="flex items-center justify-between gap-2">
          <span class="min-w-0 truncate">
            {a.label}
            <span class={hint}>({a.kind === 'editor' ? t('settings.launch_kind_editor') : t('settings.launch_kind_terminal')}) {a.exe} {a.args}</span>
          </span>
          <button type="button" class={button} aria-label={t('settings.launch_remove', { name: a.label })} onclick={() => remove(a.id)}>×</button>
        </li>
      {/each}
    </ul>
  {/if}
  <form class="grid gap-2 sm:grid-cols-2" onsubmit={(e) => { e.preventDefault(); add(); }}>
    <label class="block space-y-1">
      <span class={hint}>{t('settings.launch_kind')}</span>
      <select class={input} bind:value={kind}>
        <option value="editor">{t('settings.launch_kind_editor')}</option>
        <option value="terminal">{t('settings.launch_kind_terminal')}</option>
      </select>
    </label>
    <label class="block space-y-1">
      <span class={hint}>{t('settings.launch_name')}</span>
      <input type="text" class={input} bind:value={label} />
    </label>
    <label class="block space-y-1">
      <span class={hint}>{t('settings.launch_exe')}</span>
      <input type="text" class={input} bind:value={exe} />
    </label>
    <label class="block space-y-1">
      <span class={hint}>{t('settings.launch_args')}</span>
      <input type="text" class={input} bind:value={args} />
    </label>
    <p class="{hint} sm:col-span-2">{t('settings.launch_template_help')}</p>
    <div class="sm:col-span-2">
      <button type="submit" class={button} disabled={!canAdd}>{t('settings.launch_add')}</button>
    </div>
  </form>
</div>
