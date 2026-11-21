import type { Category, CategoryShare } from "@ledger/types";
import { categoryRoot } from "./category-tree";
import { formatAmount } from "@ledger/money";

export const UNCATEGORIZED_COLOR = "#909399";

const PALETTE = [
  "#5470c6",
  "#91cc75",
  "#fac858",
  "#ee6666",
  "#73c0de",
  "#3ba272",
  "#fc8452",
  "#9a60b4",
  "#ea7ccc",
  "#18a058",
  "#d03050",
  "#2080f0",
];

function hashId(id: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < id.length; i++) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

export function categoryColor(id: string): string {
  return PALETTE[hashId(id) % PALETTE.length];
}

export function paletteColor(index: number): string {
  return PALETTE[index % PALETTE.length];
}

export interface CategoryBar {
  id: string | null;
  name: string;
  value: number;
  color: string;
}

function toBars(
  entries: Iterable<{ id: string | null; name: string; value: number }>,
): CategoryBar[] {
  return Array.from(entries)
    .sort((a, b) => b.value - a.value)
    .map((e) => ({ ...e, color: e.id === null ? UNCATEGORIZED_COLOR : categoryColor(e.id) }));
}

export function categoryBars(shares: CategoryShare[], categories: Category[]): CategoryBar[] {
  const merged = new Map<string | null, { id: string | null; name: string; value: number }>();
  for (const s of shares) {
    if (s.amount_cents === 0) continue;
    const root = s.category_id ? categoryRoot(categories, s.category_id) : undefined;
    const id = root ? root.id : s.category_id || null;
    const name = root ? root.name : s.category_name;
    const exist = merged.get(id);
    if (exist) exist.value += s.amount_cents;
    else merged.set(id, { id, name, value: s.amount_cents });
  }
  return toBars(merged.values());
}

export function categoryDrilldownBars(
  shares: CategoryShare[],
  categories: Category[],
  rootId: string,
  directName: string,
): CategoryBar[] {
  const merged = new Map<string, { id: string; name: string; value: number }>();
  for (const s of shares) {
    if (s.amount_cents === 0) continue;
    const root = s.category_id ? categoryRoot(categories, s.category_id) : undefined;
    if (root?.id !== rootId) continue;
    const direct = s.category_id === rootId;
    const id = direct ? rootId : (s.category_id as string);
    const name = direct
      ? directName
      : (categories.find((c) => c.id === id)?.name ?? s.category_name);
    const exist = merged.get(id);
    if (exist) exist.value += s.amount_cents;
    else merged.set(id, { id, name, value: s.amount_cents });
  }
  return toBars(merged.values());
}

export function categoryBarTotal(bars: { value: number }[]): number {
  return bars.reduce((sum, b) => sum + b.value, 0);
}

export function sharePercent(value: number, total: number): number {
  if (total === 0) return 0;
  return Math.round((value / total) * 100);
}

export function barTooltipLabel(value: number, total: number): string {
  if (total === 0) return formatAmount(value);
  return `${formatAmount(value)} · ${sharePercent(value, total)}%`;
}
