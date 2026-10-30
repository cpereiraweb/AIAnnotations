<script lang="ts">
  import Layout from './components/Layout.svelte';
  import { t } from './lib/i18n';
  import { router } from './lib/router.svelte';
  import Placeholder from './routes/Placeholder.svelte';
  import Today from './routes/Today.svelte';
</script>

<Layout>
  {#if router.route.name === 'projects' || router.route.name === 'project'}
    <Placeholder title={t('nav.projects')} />
  {:else if router.route.name === 'sessions'}
    <Placeholder title={t('nav.sessions')} />
  {:else}
    <Today />
  {/if}
</Layout>
