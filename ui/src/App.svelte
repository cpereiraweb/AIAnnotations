<script lang="ts">
  import Layout from './components/Layout.svelte';
  import { router } from './lib/router.svelte';
  import Projects from './routes/Projects.svelte';
  import ProjectShow from './routes/ProjectShow.svelte';
  import Sessions from './routes/Sessions.svelte';
  import Today from './routes/Today.svelte';
</script>

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
