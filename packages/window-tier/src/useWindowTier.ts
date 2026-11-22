import { computed, onScopeDispose, ref } from "vue";
import type { ComputedRef } from "vue";

export const WINDOW_TIER_BREAKPOINT_PX = 840;

const DESKTOP_TIER_QUERY = `(min-width: ${WINDOW_TIER_BREAKPOINT_PX}px)`;

export type WindowTier = "desktop" | "mobile";

export function useWindowTier(): ComputedRef<WindowTier> {
  const mql = window.matchMedia(DESKTOP_TIER_QUERY);
  const isDesktop = ref(mql.matches);
  const onChange = (event: MediaQueryListEvent): void => {
    isDesktop.value = event.matches;
  };
  mql.addEventListener("change", onChange);
  onScopeDispose(() => mql.removeEventListener("change", onChange));
  return computed<WindowTier>(() => (isDesktop.value ? "desktop" : "mobile"));
}
