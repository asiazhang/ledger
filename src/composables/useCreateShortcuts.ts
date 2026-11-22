import { onUnmounted, watch } from "vue";
import { hasOpenOverlay } from "@ledger/ui-kit/overlayRegistry";
import { useInputMode } from "@/composables/useInputMode";
import type { CreateTransactionKind, TransactionPageCreateKind } from "@ledger/types";

export const CREATE_KIND_KEYS = {
  expense: "a",
  transfer: "z",
  income: "i",
} as const satisfies Record<TransactionPageCreateKind, string>;

export function matchCreateShortcut(e: KeyboardEvent): CreateTransactionKind | null {
  if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return null;
  const found = (Object.entries(CREATE_KIND_KEYS) as [CreateTransactionKind, string][]).find(
    ([, key]) => e.key === key,
  );
  return found?.[0] ?? null;
}

export function isEditableTarget(e: Event): boolean {
  const el = e.target;
  if (!(el instanceof HTMLElement)) return false;
  if (el.isContentEditable) return true;
  const ce = el.getAttribute("contenteditable");
  if (ce === "" || ce === "true") return true;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT";
}

export function useCreateShortcuts(
  open: (kind: CreateTransactionKind) => void,
  isKindAvailable: (kind: CreateTransactionKind) => boolean,
) {
  const onKeydown = (e: KeyboardEvent) => {
    const kind = matchCreateShortcut(e);
    if (!kind) return;
    if (!isKindAvailable(kind)) return;
    if (isEditableTarget(e) || hasOpenOverlay()) return;
    e.preventDefault();
    open(kind);
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
