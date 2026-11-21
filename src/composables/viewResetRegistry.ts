import { onScopeDispose } from "vue";

let currentReset: (() => void) | null = null;

export function registerViewReset(reset: () => void): void {
  currentReset = reset;
  onScopeDispose(() => {
    if (currentReset === reset) currentReset = null;
  });
}

export function fireViewReset(): boolean {
  if (currentReset === null) return false;
  currentReset();
  return true;
}

export function clearViewResets(): void {
  currentReset = null;
}
