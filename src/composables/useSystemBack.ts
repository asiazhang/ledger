import { onScopeDispose, watch } from "vue";
import { useRouter } from "vue-router";
import { onBackButtonPress } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { closeTopOverlay, hasOpenOverlay } from "@ledger/ui-kit/overlayRegistry";
import { useWindowTier } from "@ledger/window-tier";
import { createLatestWins } from "@ledger/latest-wins";

export function useSystemBack(): void {
  const tier = useWindowTier();
  const router = useRouter();

  let activeUnlisten: (() => void) | null = null;
  const wins = createLatestWins();

  function handleBackPress(payload: { canGoBack: boolean }): void {
    if (hasOpenOverlay()) {
      closeTopOverlay();
      return;
    }
    if (payload.canGoBack) {
      router.back();
      return;
    }
    getCurrentWindow()
      .destroy()
      .catch(() => {});
  }

  watch(
    tier,
    (value) => {
      const myToken = wins.begin();
      activeUnlisten?.();
      activeUnlisten = null;
      if (value !== "mobile") return;
      onBackButtonPress(handleBackPress)
        .then((listener) => {
          if (myToken.isStale()) {
            void listener.unregister();
            return;
          }
          activeUnlisten = () => void listener.unregister();
        })
        .catch(() => {});
    },
    { immediate: true },
  );

  onScopeDispose(() => {
    wins.invalidate();
    activeUnlisten?.();
    activeUnlisten = null;
  });
}
