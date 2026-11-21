import { ref } from "vue";
import type { Currency } from "@ledger/types";
import { currentLocale, type Locale } from "@ledger/i18n";

export const AMOUNT_PRIVACY_STORAGE_KEY = "amount_privacy_enabled";

export const amountPrivacyEnabled = ref(false);

const AMOUNT_PRIVACY_MASK = "••••";

function groupSizeFor(locale: Locale): number {
  return locale === "en-US" ? 3 : 4;
}

function joinGroups(digits: string, size: number): string {
  const groups: string[] = [];
  for (let i = digits.length; i > 0; i -= size)
    groups.unshift(digits.slice(Math.max(0, i - size), i));
  return groups.join(",");
}

function groupNumberString(numStr: string, size: number): string {
  const sign = numStr.startsWith("-") ? "-" : "";
  const body = sign ? numStr.slice(1) : numStr;
  const dot = body.indexOf(".");
  if (dot === -1) return `${sign}${joinGroups(body, size)}`;
  return `${sign}${joinGroups(body.slice(0, dot), size)}${body.slice(dot)}`;
}

const QUANTITY_DECIMALS = 4;

export function formatQuantity(quantity: number, locale: Locale = currentLocale.value): string {
  if (amountPrivacyEnabled.value) return AMOUNT_PRIVACY_MASK;
  const fixed = quantity.toFixed(QUANTITY_DECIMALS);
  const trimmed = fixed.replace(/0+$/, "").replace(/\.$/, "");
  const normalized = trimmed === "-0" ? "0" : trimmed;
  return groupNumberString(normalized, groupSizeFor(locale));
}

export function formatAmount(
  cents: number,
  currency?: Currency,
  locale: Locale = currentLocale.value,
): string {
  if (amountPrivacyEnabled.value) return AMOUNT_PRIVACY_MASK;
  const dp = currency?.decimal_places ?? 2;
  const sign = cents < 0 ? "-" : "";
  const abs = Math.abs(cents);
  const value = abs / Math.pow(10, dp);
  const fixed = value.toFixed(dp);
  const trimmed = dp > 0 ? fixed.replace(/0+$/, "").replace(/\.$/, "") : fixed;
  const symbol = currency?.symbol ?? "";
  return `${sign}${symbol}${groupNumberString(trimmed, groupSizeFor(locale))}`;
}

export function formatRate(rate: number, locale: Locale = currentLocale.value): string {
  return new Intl.NumberFormat(locale, {
    style: "percent",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
    signDisplay: "exceptZero",
  }).format(rate);
}

export function centsToYuan(cents: number, currency?: Currency): number {
  const dp = currency?.decimal_places ?? 2;
  return cents / Math.pow(10, dp);
}

export const PRICE_UNITS_PER_FEN = 100;
export const PRICE_UNITS_PER_YUAN = 10000;

export function formatPrice(
  price: number,
  currency?: Currency,
  locale: Locale = currentLocale.value,
): string {
  if (amountPrivacyEnabled.value) return AMOUNT_PRIVACY_MASK;
  const sign = price < 0 ? "-" : "";
  const abs = Math.abs(price);
  const fixed = (abs / 10000).toFixed(4);
  const trimmed = fixed.replace(/0+$/, "").replace(/\.$/, "");
  const symbol = currency?.symbol ?? "";
  return `${sign}${symbol}${groupNumberString(trimmed, groupSizeFor(locale))}`;
}

export function yuanToPrice(yuan: string | number): number | null {
  const trimmed = typeof yuan === "number" ? String(yuan) : yuan.trim();
  if (!trimmed) return null;
  if (!/^-?\d*\.?\d+$/.test(trimmed)) return null;
  const num = Number(trimmed);
  if (!Number.isFinite(num)) return null;
  const price = Math.round(Number((num * 10000).toFixed(8)));
  return Number.isSafeInteger(price) ? price : null;
}

export function priceToYuan(price: number): number {
  return price / 10000;
}

export function yuanToCents(yuan: string | number): number | null {
  const trimmed = typeof yuan === "number" ? String(yuan) : yuan.trim();
  if (!trimmed) return null;
  if (!/^-?\d*\.?\d+$/.test(trimmed)) return null;
  const num = Number(trimmed);
  if (!Number.isFinite(num)) return null;
  const cents = Math.round(Number((num * 100).toFixed(8)));
  return Number.isSafeInteger(cents) ? cents : null;
}
