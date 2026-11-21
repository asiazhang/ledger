import type { App } from "vue";
import type { Pinia } from "pinia";
import { useRenderErrorsStore } from "@/stores/render-errors";
import { api } from "@ledger/api";

const MAX_FORWARDS = 50;

const DEDUPE_WINDOW_MS = 3000;

const MESSAGE_LIMIT = 200;
const STACK_LIMIT = 600;

export function installGlobalErrorHandler(app: App, pinia: Pinia): void {
  const store = useRenderErrorsStore(pinia);

  let lastForwardedMessage: string | null = null;
  let lastForwardedAt = 0;
  let forwardedCount = 0;

  function forwardToBackend(summary: string, error: unknown): void {
    if (forwardedCount >= MAX_FORWARDS) return;
    const now = Date.now();
    if (summary === lastForwardedMessage && now - lastForwardedAt < DEDUPE_WINDOW_MS) return;
    lastForwardedMessage = summary;
    lastForwardedAt = now;
    forwardedCount += 1;
    const err = error as { stack?: string } | null;
    const stack = typeof err?.stack === "string" ? err.stack.slice(0, STACK_LIMIT) : "";
    api.logFrontendError(stack ? `${summary}\n${stack}` : summary).catch(() => {});
  }

  app.config.errorHandler = (err, _instance, info) => {
    const raw = err instanceof Error ? err.message : String(err);
    const summary = `${raw}（${info}）`.slice(0, MESSAGE_LIMIT);
    store.report(summary);
    forwardToBackend(summary, err);
  };
}
