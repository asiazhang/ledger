import { onMounted, onUnmounted } from "vue";
import { isEditableTarget } from "@/composables/useCreateShortcuts";
import { hasOpenOverlay } from "@ledger/ui-kit/overlayRegistry";
import { fireViewReset } from "@/composables/viewResetRegistry";

export function useWindowGuard() {
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    e.preventDefault();
    if (hasOpenOverlay()) return;
    fireViewReset();
  };
  const onContextMenu = (e: MouseEvent) => {
    if (isEditableTarget(e)) return;
    e.preventDefault();
  };
  onMounted(() => {
    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("contextmenu", onContextMenu, true);
  });
  onUnmounted(() => {
    document.removeEventListener("keydown", onKeyDown, true);
    document.removeEventListener("contextmenu", onContextMenu, true);
  });
}
