import { ref } from "vue";
import { errorMessage } from "@ledger/utils/errors";
import { createLatestWins } from "@ledger/latest-wins";

export interface ToastSink {
  error: (content: string) => void;
}

let toastSink: ToastSink = { error: () => {} };

export function registerToastSink(sink: ToastSink): void {
  toastSink = sink;
}

function showErrorToast(message: string): void {
  toastSink.error(message);
}

export function toastError(content: string): void {
  showErrorToast(content);
}

export interface UseLoadableOptions {
  silent?: boolean;
}

export function useLoadable<T>(task: () => Promise<T>, options: UseLoadableOptions = {}) {
  const silent = options.silent ?? false;
  const loading = ref(false);
  const error = ref<string | null>(null);

  const wins = createLatestWins();

  async function run(): Promise<T | null> {
    const myToken = wins.begin();
    loading.value = true;
    try {
      const result = await task();
      if (myToken.isStale()) return null;
      error.value = null;
      return result;
    } catch (e) {
      if (myToken.isStale()) return null;
      error.value = errorMessage(e);
      if (!silent) showErrorToast(error.value);
      return null;
    } finally {
      if (!myToken.isStale()) loading.value = false;
    }
  }

  function invalidate(): void {
    wins.invalidate();
    loading.value = false;
  }

  return { loading, error, run, invalidate };
}
