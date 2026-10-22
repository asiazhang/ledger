import type {
  Budget,
  BudgetAnnualSummary,
  BudgetInput,
  BudgetProgress,
  BudgetUpdateInput,
} from "@ledger/types";
import {
  DEVICE_ID,
  MOCK_TODAY,
  db,
  emitLedgerChanged,
  findActive,
  nextId,
  nowIso,
  obj,
  softDelete,
  str,
  touch,
} from "./state";

/**
 * 预算域替身（issue #1967 / spec #1964 / ADR-0143）：内存态 CRUD + 夹具直聚合，
 * 预算页「建预算 → 列表出现 → 进度变化」闭环。从简口径（不追行为等价，与
 * 真后端的差异点，发现误导 UI 开发时以真后端为准修替身）：
 *
 * - **聚合窗口锚定字面量「今日」2026-01-15**（当月 = 2026-01、本年 = 2026）：
 *   夹具交易全部落在该窗口内，进度条有数可看且期望值确定（确定性纪律）；
 *   真后端按运行日滚动当前自然月/年，替身不随真实时钟走。
 * - **spent = 当期该分类 kind=expense 交易金额直和**：不冲减退款（真后端为
 *   expense_net 毛支出 − 退款）、不展开父分类预算覆盖、不折算（夹具单币种
 *   CNY 直记，amount_native_cents 即默认币种口径）。
 * - **年度汇总本年已支只计「预算覆盖分类」的支出**（真后端同边界，未设预算
 *   分类的支出不进汇总），按交易逐笔记一次——父子预算并存不重复计。
 * - **校验只做存在性必需**（分类在用、预算行在用）：金额正数、「分类 + 周期」
 *   唯一、仅支出分类可设预算等真后端校验不模拟（spec #1964 取向）。
 * - update 的分类不可改语义从简：input 契约本身无该字段，替身无额外护栏。
 *
 * 写路径（create / update / delete）完成后统一发 `ledger:changed`（ADR-0143
 * 决策 3：事件驱动失效是预算所属参考 store 的重拉触发源）。
 */

const CURRENT_MONTH = MOCK_TODAY.slice(0, 7); // "2026-01"
const CURRENT_YEAR = MOCK_TODAY.slice(0, 4); // "2026"

/** 在用预算行，按创建先后排序（与真后端 list_budgets 同序）。 */
function activeBudgets(): Budget[] {
  return db()
    .budgets.filter((b) => !b.is_deleted)
    .sort((a, b) => a.created_at.localeCompare(b.created_at));
}

export function listBudgets(): Budget[] {
  return activeBudgets();
}

export function createBudget(args: Record<string, unknown>): string {
  const input = obj<BudgetInput>(args, "input");
  // 存在性校验（spec #1964 取向）：分类必须在用
  findActive(db().categories, input.category_id, "分类");
  const row: Budget = {
    id: nextId("mock-bud"),
    category_id: input.category_id,
    period: input.period ?? "monthly",
    amount_cents: input.amount_cents,
    start_date: input.start_date,
    created_at: nowIso(),
    updated_at: nowIso(),
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
  db().budgets.push(row);
  emitLedgerChanged();
  return row.id;
}

export function updateBudget(args: Record<string, unknown>): void {
  const row = findActive(db().budgets, str(args, "id"), "预算");
  const input = obj<BudgetUpdateInput>(args, "input");
  row.amount_cents = input.amount_cents;
  if (input.period !== undefined) row.period = input.period; // 缺省 = 不改周期
  touch(row);
  emitLedgerChanged();
}

export function deleteBudget(args: Record<string, unknown>): void {
  softDelete(db().budgets, str(args, "id"), "预算");
  emitLedgerChanged();
}

export function budgetProgress(): BudgetProgress[] {
  const categories = db().categories;
  return activeBudgets().map((b) => {
    const window = b.period === "monthly" ? CURRENT_MONTH : CURRENT_YEAR; // date 前缀即自然周期
    const spent = db()
      .transactions.filter(
        (t) =>
          !t.is_deleted &&
          t.kind === "expense" &&
          t.category_id === b.category_id &&
          t.date.startsWith(window),
      )
      .reduce((sum, t) => sum + t.amount_native_cents, 0);
    return {
      budget: b,
      category_name: categories.find((c) => c.id === b.category_id)?.name ?? "",
      spent_cents: spent,
      over_budget: spent > b.amount_cents,
    };
  });
}

export function budgetAnnualSummary(): BudgetAnnualSummary {
  const budgets = activeBudgets();
  // 年化总额 = Σ 月预算 × 12 + Σ 年预算（真后端同名口径：无窗口、不滚动）
  const total = budgets.reduce(
    (sum, b) => sum + (b.period === "monthly" ? b.amount_cents * 12 : b.amount_cents),
    0,
  );
  // 本年已支：预算覆盖分类的本年 expense 直和，按交易逐笔记一次（未设预算分类不计入）
  const covered = new Set(budgets.map((b) => b.category_id));
  const spent = db()
    .transactions.filter(
      (t) =>
        !t.is_deleted &&
        t.kind === "expense" &&
        t.category_id !== null &&
        covered.has(t.category_id) &&
        t.date.startsWith(CURRENT_YEAR),
    )
    .reduce((sum, t) => sum + t.amount_native_cents, 0);
  return {
    annual_budget_total_cents: total,
    spent_cents: spent,
    over_budget: spent > total,
  };
}
