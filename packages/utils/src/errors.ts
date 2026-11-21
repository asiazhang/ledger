import { currentLocale, i18n } from "@ledger/i18n";

function extractCode(e: unknown): { code: string; params: string[] } | null {
  if (typeof e !== "object" || e === null) return null;
  const code = (e as { code?: unknown }).code;
  if (typeof code !== "string" || !code) return null;
  const raw = (e as { params?: unknown }).params;
  const params = Array.isArray(raw) ? raw.filter((p): p is string => typeof p === "string") : [];
  return { code, params };
}

export function errorCodeOf(e: unknown): string | null {
  return extractCode(e)?.code ?? null;
}

function placeholderArity(key: string): number {
  const raw = i18n.global.tm(key);
  if (typeof raw !== "string") return 0;
  const indices = [...raw.matchAll(/\{(\d+)\}/g)].map((m) => Number(m[1]));
  return indices.length > 0 ? Math.max(...indices) + 1 : 0;
}

export function errorMessage(e: unknown): string {
  const coded = extractCode(e);
  if (coded) {
    const key = `errors.${coded.code}`;
    if (i18n.global.te(key, currentLocale.value) && coded.params.length >= placeholderArity(key)) {
      return coded.params.length > 0 ? i18n.global.t(key, coded.params) : i18n.global.t(key);
    }
  }
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  if (typeof e === "object" && e !== null && "message" in e) {
    const message = (e as { message: unknown }).message;
    if (typeof message === "string" && message) return message;
  }
  return String(e);
}
