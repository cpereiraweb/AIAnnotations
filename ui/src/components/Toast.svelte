<script lang="ts">
  import { toastState } from '../lib/toast.svelte';
</script>

<!-- Always in the DOM so screen readers announce the changes (aria-live). -->
<div
  role="status"
  aria-live="polite"
  class="pointer-events-none fixed bottom-5 left-1/2 -translate-x-1/2 rounded-md bg-stone-800 px-3 py-1.5 text-xs text-stone-50 shadow-sm transition-opacity dark:bg-stone-200 dark:text-stone-900 {toastState.message
    ? 'opacity-100'
    : 'opacity-0'}"
>{toastState.message}</div>
