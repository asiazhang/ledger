import { onUnmounted, watch } from "vue";
import { hasOpenOverlay } from "@ledger/ui-kit/overlayRegistry";
import { useInputMode } from "@/composables/useInputMode";
import { isEditableTarget } from "@/composables/useCreateShortcuts";

export function useBareKeyShortcuts<T>(
  match: (e: KeyboardEvent) => T | null,
  dispatch: (hit: T) => void,
): void {
  const onKeydown = (e: KeyboardEvent) => {
    const hit = match(e);
    if (hit === null) return;
    if (isEditableTarget(e) || hasOpenOverlay()) return;
    e.preventDefault();
    dispatch(hit);
  };
  const inputMode = useInputMode();
  watch(
    inputMode,
    (mode) => {
      if (mode === "pointer") window.addEventListener("keydown", onKeydown);
      else window.removeEventListener("keydown", onKeydown);
    },
    { immediate: true },
  );
  onUnmounted(() => window.removeEventListener("keydown", onKeydown));
}
