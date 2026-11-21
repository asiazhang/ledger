import { onUnmounted } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const PRICES_CHANGED_EVENT = "ledger:prices-changed";

export function usePricesChanged(callback: () => void): void {
  let unlisten: UnlistenFn | null = null;
  let disposed = false;
  void listen(PRICES_CHANGED_EVENT, callback)
    .then((fn) => {
      if (disposed) {
        fn();
        return;
      }
      unlisten = fn;
    })
    .catch((e) => {
      console.warn(`订阅 ${PRICES_CHANGED_EVENT} 失败`, e);
    });
  onUnmounted(() => {
    disposed = true;
    unlisten?.();
    unlisten = null;
  });
}
