import { computed, onScopeDispose, ref } from "vue";
import type { ComputedRef } from "vue";

export const HOVER_QUERY = "(hover: hover)";
export const FINE_POINTER_QUERY = "(pointer: fine)";

export type InputMode = "touch" | "pointer";

const canHover = ref(false);
const hasFinePointer = ref(false);

const onHoverChange = (event: MediaQueryListEvent): void => {
  canHover.value = event.matches;
};
const onPointerChange = (event: MediaQueryListEvent): void => {
  hasFinePointer.value = event.matches;
};

let boundHoverMql: MediaQueryList | null = null;
let boundPointerMql: MediaQueryList | null = null;
let consumerCount = 0;

function bindSharedListeners(): void {
  const hoverMql = window.matchMedia(HOVER_QUERY);
  const finePointerMql = window.matchMedia(FINE_POINTER_QUERY);
  if (boundHoverMql) boundHoverMql.removeEventListener("change", onHoverChange);
  if (boundPointerMql) boundPointerMql.removeEventListener("change", onPointerChange);
  boundHoverMql = hoverMql;
  boundPointerMql = finePointerMql;
  hoverMql.addEventListener("change", onHoverChange);
  finePointerMql.addEventListener("change", onPointerChange);
  canHover.value = hoverMql.matches;
  hasFinePointer.value = finePointerMql.matches;
}

function releaseSharedListeners(): void {
  consumerCount -= 1;
  if (consumerCount > 0) return;
  boundHoverMql?.removeEventListener("change", onHoverChange);
  boundPointerMql?.removeEventListener("change", onPointerChange);
  boundHoverMql = null;
  boundPointerMql = null;
}

export const sharedInputMode: ComputedRef<InputMode> = computed<InputMode>(() =>
  canHover.value && hasFinePointer.value ? "pointer" : "touch",
);

export function useInputMode(): ComputedRef<InputMode> {
  consumerCount += 1;
  bindSharedListeners();
  onScopeDispose(releaseSharedListeners);
  return computed<InputMode>(() => (canHover.value && hasFinePointer.value ? "pointer" : "touch"));
}
