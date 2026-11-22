import { readonly, ref } from "vue";

export const BUSY_SHOW_DELAY_MS = 300;

const pendingCount = ref(0);
const visible = ref(false);

let showTimer: ReturnType<typeof setTimeout> | null = null;

function clearShowTimer(): void {
  if (showTimer !== null) {
    clearTimeout(showTimer);
    showTimer = null;
  }
}

export const busyVisible = readonly(visible);

export function trackBusy<T>(task: Promise<T>): Promise<T> {
  pendingCount.value++;
  if (showTimer === null) {
    showTimer = setTimeout(() => {
      showTimer = null;
      if (pendingCount.value > 0) visible.value = true;
    }, BUSY_SHOW_DELAY_MS);
  }
  const end = () => {
    pendingCount.value--;
    if (pendingCount.value === 0) {
      clearShowTimer();
      visible.value = false;
    }
  };
  const p = Promise.resolve(task);
  p.then(end, end);
  return p;
}

export function resetGlobalBusy(): void {
  clearShowTimer();
  pendingCount.value = 0;
  visible.value = false;
}
