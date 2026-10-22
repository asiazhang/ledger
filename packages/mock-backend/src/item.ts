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

/**
 * 物品域替身（issue #1974 / spec #1964 / ADR-0143）：物品档案内存态读写闭环，
 * 物品页「建档 → 列表出现 → 处置 → 日均成本重锚」可用。从简口径（不追行为
 * 等价，与真后端的差异点，发现误导 UI 开发时以真后端为准修替身）：
 *
 * - **日均成本三元组与真后端 `item::cost` 接缝同口径**：分母 = 购买日 → 目标日
 *   的日历天数**含起止两端**（同日 = 1）；分子 = 总成本 − 残值，下限 0；
 *   每天成本 = 分子 ÷ 天数（小数，仅供展示）。列表缺省目标日：在用 → 替身
 *   今日（MOCK_TODAY 字面量锚，真后端为本地时区今天）；已处置 → 处置日
 *   （不随今日漂移）。
 * - **`cost_native_cents` 原币直记**（= total_cost_cents）：替身不折算（ADR-0143
 *   决策 1；真后端经 Amount 接缝按交易日折算本位币）。
 * - **购买溯源从简**：创建不强制关联购买交易（真后端溯源守卫必填 expense 且
 *   自动带出日期/成本/币种）；带关联即校验交易在用，入参字段照单落行
 *   （UI 表单本就按交易预填，结果同形）。修改提供新指针 = 换关，缺省 =
 *   保留既有指针（真后端「溯源只增不减」同语义）。
 * - **校验只做存在性必需**（物品行在用、关联交易在用）+ 处置日期序护栏
 *   （处置日早于购买日 / 晚于替身今日即拒绝，防列表读取时三元组算不出）
 *   ——名称非空、成本正数、残值非负等真后端校验不模拟（spec #1964 取向）。
 *
 * 写路径（create / update / dispose / delete）完成后统一发 `ledger:changed`
 * （ADR-0143 决策 3：事件驱动失效是物品 store 的重拉触发源）。
 */

/** 在用物品行，按创建先后排序（与真后端 list_items 同序）。 */
function activeItems(): Item[] {
  return db()
    .items.filter((i) => !i.is_deleted)
    .sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id));
}

/** UTC 日历天数差（YYYY-MM-DD 字面量：解析与差值均在 UTC 轴，无时区漂移）。 */
function daysBetween(from: string, to: string): number {
  return Math.round((Date.parse(to) - Date.parse(from)) / 86_400_000);
}

/** 三元组本体（`item::cost` 接缝同口径）：目标日早于购买日报错，不静默回绕。 */
function usageTo(item: Item, targetDate: string): ItemDailyCost {
  const days = daysBetween(item.purchase_date, targetDate) + 1; // 含起止两端
  if (days <= 0)
    throw new Error(`替身后端：目标日期 ${targetDate} 早于购买日期 ${item.purchase_date}`);
  const numeratorCents = Math.max(item.total_cost_cents - (item.residual_value_cents ?? 0), 0);
  return {
    used_days: days,
    numerator_cents: numeratorCents,
    per_day_cents: numeratorCents / days,
  };
}

/** 列表缺省目标日（在用 → 替身今日；已处置 → 处置日）。 */
function dailyUsage(item: Item): ItemDailyCost {
  if (item.status === "in_use") return usageTo(item, MOCK_TODAY);
  if (item.disposal_date === null) throw new Error(`替身后端：已处置物品缺少处置日期 ${item.id}`);
  return usageTo(item, item.disposal_date);
}

export function listItems(): ItemWithDailyCost[] {
  return activeItems().map((item) => ({ ...item, ...dailyUsage(item) }));
}

/** 自选参考日重算（issue #121）：缺省沿用列表口径，提供即覆盖目标日（含未来日预览）。 */
export function calculateItemCost(args: Record<string, unknown>): ItemDailyCost {
  const id = str(args, "id");
  const item = db().items.find((i) => i.id === id && !i.is_deleted);
  if (!item) throw new Error(`替身后端：物品不存在或已删除 ${id}`);
  const referenceDate = args.referenceDate;
  return typeof referenceDate === "string" && referenceDate !== ""
    ? usageTo(item, referenceDate)
    : dailyUsage(item);
}

/** 关联购买交易的存在性校验（带关联即校验；不强制、不自动带出，见头注）。 */
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
    cost_native_cents: input.total_cost_cents, // 原币直记，不折算（见头注）
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
  row.purchase_transaction_id = link; // 缺省 = 保留既有指针（溯源只增不减）
  row.note = input.note ?? null;
  touch(row);
  emitLedgerChanged();
}

/** 处置（issue #120）：置 disposed + 处置日 + 残值；再次处置 = 修正处置信息。 */
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
