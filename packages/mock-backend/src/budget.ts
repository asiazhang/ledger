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

const CURRENT_MONTH = MOCK_TODAY.slice(0, 7);
const CURRENT_YEAR = MOCK_TODAY.slice(0, 4);

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
  if (input.period !== undefined) row.period = input.period;
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
    const window = b.period === "monthly" ? CURRENT_MONTH : CURRENT_YEAR;
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
  const total = budgets.reduce(
    (sum, b) => sum + (b.period === "monthly" ? b.amount_cents * 12 : b.amount_cents),
    0,
  );
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
