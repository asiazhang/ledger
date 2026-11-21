import { useBareKeyShortcuts } from "@/composables/useBareKeyShortcuts";

export const PERIOD_STEP_KEYS = {
  prev: ",",
  next: ".",
} as const;

export type PeriodStep = "prev" | "next";

export function matchPeriodStepShortcut(e: KeyboardEvent): PeriodStep | null {
  if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return null;
  if (e.key === PERIOD_STEP_KEYS.prev) return "prev";
  if (e.key === PERIOD_STEP_KEYS.next) return "next";
  return null;
}

export function useTimePeriodShortcuts(step: (dir: PeriodStep) => void): void {
  useBareKeyShortcuts(matchPeriodStepShortcut, step);
}
