<script lang="ts">
  import { onMount } from 'svelte';
  import Layout from './components/Layout.svelte';
  import { router } from './lib/router.svelte';
  import Projects from './routes/Projects.svelte';
  import ProjectShow from './routes/ProjectShow.svelte';
  import Sessions from './routes/Sessions.svelte';
  import Onboarding from './routes/Onboarding.svelte';
  import Today from './routes/Today.svelte';
  import Toast from './components/Toast.svelte';
  import { NO_BACKEND, describeError, onboarding_state, onNavigate } from './lib/api';
  import { setLang, t } from './lib/i18n';

  // `null` until the backend answered. Without a backend, behave as if the wizard was done.
  let onboardingDone = $state<boolean | null>(null);
  let loadError = $state<string | null>(null);

  function loadState() {
    loadError = null;
    onboarding_state().then(
      (v) => {
        setLang(v.lang);
        document.documentElement.lang = v.lang;
        onboardingDone = v.done;
      },
      (e) => {
        // Any other failure must not skip the wizard (the AI would stay silently off).
        if (e instanceof Error && e.message === NO_BACKEND) onboardingDone = true;
        else loadError = describeError(e);
      },
    );
  }

  onMount(loadState);

  function finished() {
    onboardingDone = true;
    window.location.hash = '#/';
  }

  // The tray's "Hoje" shows the window and asks for Today.
  onMount(() =>
    onNavigate((hash) => {
      window.location.hash = hash;
    }),
  );
</script>

{#if loadError}
  <div class="mx-auto max-w-2xl space-y-3 px-6 py-8 text-[13px]">
    <p class="text-red-600 dark:text-red-400">{loadError}</p>
    <button
      type="button"
      class="rounded border border-stone-300 px-3 py-1 text-xs text-stone-700 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800"
      onclick={loadState}
    >{t('onboarding.retry')}</button>
  </div>
{:else if onboardingDone === null}
  <!-- Waiting for the first answer. -->
{:else if !onboardingDone || router.route.name === 'onboarding'}
  <Onboarding onfinish={finished} />
  <Toast />
{:else}
<Layout>
  {#if router.route.name === 'projects'}
    <Projects />
  {:else if router.route.name === 'project'}
    {#key router.route.id}
      <ProjectShow id={router.route.id} />
    {/key}
  {:else if router.route.name === 'sessions'}
    <Sessions />
  {:else}
    <Today />
  {/if}
</Layout>
{/if}
