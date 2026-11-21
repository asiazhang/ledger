import type { Item, ItemDailyCost, ItemInput, ItemWithDailyCost } from "@ledger/types";
import {
  DEVICE_ID,
  MOCK_TODAY,
  db,
  emitLedgerChanged,
  nextId,
  nowIso,
  obj,
  softDelete,
  str,
  touch,
} from "./state";

function activeItems(): Item[] {
  return db()
    .items.filter((i) => !i.is_deleted)
    .sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id));
}

function daysBetween(from: string, to: string): number {
  return Math.round((Date.parse(to) - Date.parse(from)) / 86_400_000);
}

function usageTo(item: Item, targetDate: string): ItemDailyCost {
  const days = daysBetween(item.purchase_date, targetDate) + 1;
  if (days <= 0)
    throw new Error(`替身后端：目标日期 ${targetDate} 早于购买日期 ${item.purchase_date}`);
  const numeratorCents = Math.max(item.total_cost_cents - (item.residual_value_cents ?? 0), 0);
  return {
    used_days: days,
    numerator_cents: numeratorCents,
    per_day_cents: numeratorCents / days,
  };
}

function dailyUsage(item: Item): ItemDailyCost {
  if (item.status === "in_use") {
    return usageTo(item, MOCK_TODAY > item.purchase_date ? MOCK_TODAY : item.purchase_date);
  }
  if (item.disposal_date === null) throw new Error(`替身后端：已处置物品缺少处置日期 ${item.id}`);
  return usageTo(item, item.disposal_date);
}

export function listItems(): ItemWithDailyCost[] {
  return activeItems().map((item) => ({ ...item, ...dailyUsage(item) }));
}

export function calculateItemCost(args: Record<string, unknown>): ItemDailyCost {
  const id = str(args, "id");
  const item = db().items.find((i) => i.id === id && !i.is_deleted);
  if (!item) throw new Error(`替身后端：物品不存在或已删除 ${id}`);
  const referenceDate = args.referenceDate;
  return typeof referenceDate === "string" && referenceDate !== ""
    ? usageTo(item, referenceDate)
    : dailyUsage(item);
}

function resolvePurchaseLink(input: ItemInput): string | null {
  if (input.purchase_transaction_id == null) return null;
  const tx = db().transactions.find((t) => t.id === input.purchase_transaction_id && !t.is_deleted);
  if (!tx) throw new Error(`替身后端：关联交易不存在或已删除 ${input.purchase_transaction_id}`);
  return tx.id;
}

export function createItem(args: Record<string, unknown>): string {
  const input = obj<ItemInput>(args, "input");
  resolvePurchaseLink(input);
  const row: Item = {
    id: nextId("mock-item"),
    name: input.name,
    purchase_date: input.purchase_date,
    total_cost_cents: input.total_cost_cents,
    currency_code: input.currency_code,
    cost_native_cents: input.total_cost_cents,
    status: "in_use",
    disposal_date: null,
    residual_value_cents: null,
    purchase_transaction_id: input.purchase_transaction_id ?? null,
    note: input.note ?? null,
    created_at: nowIso(),
    updated_at: nowIso(),
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
  db().items.push(row);
  emitLedgerChanged();
  return row.id;
}

export function updateItem(args: Record<string, unknown>): void {
  const id = str(args, "id");
  const row = db().items.find((i) => i.id === id && !i.is_deleted);
  if (!row) throw new Error(`替身后端：物品不存在或已删除 ${id}`);
  const input = obj<ItemInput>(args, "input");
  const link =
    input.purchase_transaction_id != null
      ? resolvePurchaseLink(input)
      : row.purchase_transaction_id;
  row.name = input.name;
  row.purchase_date = input.purchase_date;
  row.total_cost_cents = input.total_cost_cents;
  row.currency_code = input.currency_code;
  row.cost_native_cents = input.total_cost_cents;
  row.purchase_transaction_id = link;
  row.note = input.note ?? null;
  touch(row);
  emitLedgerChanged();
}

export function disposeItem(args: Record<string, unknown>): void {
  const id = str(args, "id");
  const row = db().items.find((i) => i.id === id && !i.is_deleted);
  if (!row) throw new Error(`替身后端：物品不存在或已删除 ${id}`);
  const input = obj<{ disposal_date: string; residual_value_cents?: number | null }>(args, "input");
  const disposalDate = str(input as unknown as Record<string, unknown>, "disposal_date");
  if (disposalDate < row.purchase_date)
    throw new Error(`替身后端：处置日期早于购买日期 ${disposalDate}`);
  if (disposalDate > MOCK_TODAY) throw new Error(`替身后端：处置日期不能晚于今天 ${disposalDate}`);
  row.status = "disposed";
  row.disposal_date = disposalDate;
  row.residual_value_cents = input.residual_value_cents ?? null;
  touch(row);
  emitLedgerChanged();
}

export function deleteItem(args: Record<string, unknown>): void {
  softDelete(db().items, str(args, "id"), "物品");
  emitLedgerChanged();
}
