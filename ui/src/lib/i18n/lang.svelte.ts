import type { Lang } from '../types';

/** The current UI language as reactive state: templates calling `t()` re-render when it changes. */
export const langState = $state<{ lang: Lang | null }>({ lang: null });
