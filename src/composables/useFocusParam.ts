import type { LocationQuery } from "vue-router";

const FOCUS_QUERY_KEY = "focus";

export interface UseFocusParamOptions {
  query: () => LocationQuery;
  onFocus: (entityId: string) => void;
}

export interface UseFocusParamReturn {
  consume(): void;
}

export function useFocusParam(options: UseFocusParamOptions): UseFocusParamReturn {
  let spent = false;

  return {
    consume() {
      if (spent) return;
      spent = true;
      const entityId = readFocusId(options.query());
      if (entityId !== null) options.onFocus(entityId);
    },
  };
}

function readFocusId(query: LocationQuery): string | null {
  const raw = query[FOCUS_QUERY_KEY];
  const value = Array.isArray(raw) ? raw[0] : raw;
  return typeof value === "string" && value !== "" ? value : null;
}

export function hasFocusParam(query: LocationQuery): boolean {
  return readFocusId(query) !== null;
}
