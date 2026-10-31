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
  import { onboarding_state, onNavigate } from './lib/api';

  // `null` until the backend answered. Without a backend, behave as if the wizard was done.
  let onboardingDone = $state<boolean | null>(null);

  onMount(() => {
    onboarding_state().then(
      (v) => (onboardingDone = v.done),
      () => (onboardingDone = true),
    );
  });

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

{#if onboardingDone === null}
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
