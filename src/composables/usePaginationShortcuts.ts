import { useBareKeyShortcuts } from "@/composables/useBareKeyShortcuts";

const PAGINATION_KEYS = {
  prev: "[",
  next: "]",
} as const;

export type PaginationStep = "prev" | "next";

function matchPaginationShortcut(e: KeyboardEvent): PaginationStep | null {
  if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return null;
  if (e.key === PAGINATION_KEYS.prev) return "prev";
  if (e.key === PAGINATION_KEYS.next) return "next";
  return null;
}

export function usePaginationShortcuts(step: (dir: PaginationStep) => void): void {
  useBareKeyShortcuts(matchPaginationShortcut, step);
}
