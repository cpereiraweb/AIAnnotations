// Global toast (Inv §1): one message at a time, visible for 1.8 s.
export const TOAST_MS = 1800;

export const toastState = $state<{ message: string }>({ message: '' });

let timer: ReturnType<typeof setTimeout> | null = null;

export function toast(message: string): void {
  toastState.message = message;
  if (timer) clearTimeout(timer);
  timer = setTimeout(() => {
    toastState.message = '';
    timer = null;
  }, TOAST_MS);
}
