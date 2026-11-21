import { ref } from "vue";
import { createI18n } from "vue-i18n";
import { loadLocal, saveLocal } from "@ledger/storage";
import zhCN from "./locales/zh-CN";

export type Locale = "zh-CN" | "en-US";
export type LocaleSetting = Locale | "system";

const LOCALE_STORAGE_KEY = "locale";
const SOURCE_LOCALE: Locale = "zh-CN";

export const currentLocale = ref<Locale>("zh-CN");

function resolveSystemLocale(): Locale {
  const lang = typeof navigator !== "undefined" ? navigator.language : "";
  return lang && lang.toLowerCase().startsWith("en") ? "en-US" : "zh-CN";
}

function resolveLocale(setting: LocaleSetting): Locale {
  return setting === "system" ? resolveSystemLocale() : setting;
}

export const i18n = createI18n({
  legacy: false,
  locale: "zh-CN" as Locale,
  fallbackLocale: SOURCE_LOCALE,
  messages: { "zh-CN": zhCN } as Record<Locale, typeof zhCN>,
  missing: (locale, key) => {
    if (import.meta.env.DEV) {
      console.warn(`[i18n] 缺失文案 key：${key}（locale=${String(locale)}）`);
    }
  },
});

async function loadLocaleBundle(locale: Locale): Promise<void> {
  if (locale === SOURCE_LOCALE || i18n.global.availableLocales.includes(locale)) return;
  const messages = await import(`./locales/${locale}/index.ts`);
  i18n.global.setLocaleMessage(locale, messages.default);
}

export async function applyLocale(locale: Locale): Promise<void> {
  await loadLocaleBundle(locale);
  i18n.global.locale.value = locale;
  currentLocale.value = locale;
}

export function getLocaleSetting(): LocaleSetting {
  return loadLocal<LocaleSetting>(LOCALE_STORAGE_KEY, "system");
}

export async function setLocaleSetting(setting: LocaleSetting): Promise<void> {
  saveLocal(LOCALE_STORAGE_KEY, setting);
  await applyLocale(resolveLocale(setting));
}

export async function initAppLocale(): Promise<void> {
  await applyLocale(resolveLocale(getLocaleSetting()));
}

export function t(key: string, params?: Record<string, unknown> | unknown[]): string {
  if (params === undefined) return i18n.global.t(key);
  if (Array.isArray(params)) return i18n.global.t(key, params);
  return i18n.global.t(key, params);
}
