import type {
  Account,
  SavingsGoal,
  SavingsGoalInput,
  SavingsGoalProgress,
  SavingsGoalPaceSource,
  SavingsGoalUpdateInput,
} from "@ledger/types";
import { accountBalanceCents } from "./ledger";
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

interface Projection {
  eta_months: number | null;
  eta_month: string | null;
  required_monthly_cents: number | null;
  pace_delta_cents: number | null;
}

function divCeil(a: number, b: number): number {
  return Math.ceil(a / b);
}

function monthIndex(date: string): number {
  const [year, month] = date.split("-");
  return Number(year) * 12 + Number(month);
}

function addMonths(yyyyMm: string, months: number): string {
  const [year, month] = yyyyMm.split("-").map(Number);
  const total = year * 12 + (month - 1) + months;
  const y = Math.floor(total / 12);
  const m = (total % 12) + 1;
  return `${String(y).padStart(4, "0")}-${String(m).padStart(2, "0")}`;
}

function project(deadline: string | null, remainingCents: number, pace: number | null): Projection {
  const projection: Projection = {
    eta_months: null,
    eta_month: null,
    required_monthly_cents: null,
    pace_delta_cents: null,
  };
  if (remainingCents <= 0) return projection;
  if (deadline === null && pace !== null && pace > 0) {
    const months = divCeil(remainingCents, pace);
    projection.eta_months = months;
    projection.eta_month = addMonths(MOCK_TODAY.slice(0, 7), months);
    return projection;
  }
  if (deadline !== null && deadline >= MOCK_TODAY) {
    const monthsLeft = Math.max(monthIndex(deadline) - monthIndex(MOCK_TODAY), 1);
    const required = divCeil(remainingCents, monthsLeft);
    projection.required_monthly_cents = required;
    if (pace !== null) projection.pace_delta_cents = pace - required;
  }
  return projection;
}

export function savingsGoalProgress(): SavingsGoalProgress[] {
  const goals = db()
    .savings_goals.filter((g) => !g.is_deleted)
    .sort((a, b) => a.created_at.localeCompare(b.created_at));
  return goals.map((goal) => {
    const account = findActive(db().accounts, goal.account_id, "专属账户");
    const saved = accountBalanceCents(goal.account_id);
    const remaining = goal.target_amount_cents - saved;
    const pace = goal.planned_monthly_cents;
    const paceSource: SavingsGoalPaceSource | null = pace !== null ? "manual" : null;
    const projection = project(goal.deadline, remaining, pace);
    return {
      goal,
      saved_cents: saved,
      remaining_cents: remaining,
      achieved: saved >= goal.target_amount_cents,
      currency_code: account.currency_code,
      pace_monthly_cents: pace,
      pace_source: paceSource,
      ...projection,
    };
  });
}

export function updateSavingsGoal(args: Record<string, unknown>): void {
  const row = activeGoal(str(args, "id"));
  const input = obj<SavingsGoalUpdateInput>(args, "input");
  const name = str(input as unknown as Record<string, unknown>, "name");
  if (typeof input.target_amount_cents !== "number") {
    throw new Error("替身后端：命令参数 target_amount_cents 缺失");
  }
  row.name = name;
  row.target_amount_cents = input.target_amount_cents;
  row.deadline = input.deadline ?? null;
  row.planned_monthly_cents = input.planned_monthly_cents ?? null;
  const account = findActive(db().accounts, row.account_id, "专属账户");
  if (account.name !== name) {
    account.name = name;
    touch(account);
  }
  touch(row);
  emitLedgerChanged();
}

export function createSavingsGoal(args: Record<string, unknown>): string {
  const input = obj<SavingsGoalInput>(args, "input");
  const name = str(input as unknown as Record<string, unknown>, "name");
  if (typeof input.target_amount_cents !== "number") {
    throw new Error("替身后端：命令参数 target_amount_cents 缺失");
  }
  const account: Account = {
    id: nextId("mock-acc"),
    name,
    type: "other",
    currency_code: db().settings.base_currency,
    initial_balance_cents: 0,
    credit_limit_cents: null,
    statement_day: null,
    due_day: null,
    created_at: nowIso(),
    is_hidden: false,
    is_archived: false,
    updated_at: nowIso(),
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
  db().accounts.push(account);
  const row: SavingsGoal = {
    id: nextId("mock-goal"),
    name,
    target_amount_cents: input.target_amount_cents,
    deadline: input.deadline ?? null,
    status: "active",
    planned_monthly_cents: null,
    account_id: account.id,
    created_at: nowIso(),
    updated_at: nowIso(),
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
  db().savings_goals.push(row);
  emitLedgerChanged();
  return row.id;
}

export function archiveSavingsGoal(args: Record<string, unknown>): void {
  const row = activeGoal(str(args, "id"));
  row.status = "archived";
  touch(row);
  emitLedgerChanged();
}

export function unarchiveSavingsGoal(args: Record<string, unknown>): void {
  const row = activeGoal(str(args, "id"));
  row.status = "active";
  touch(row);
  emitLedgerChanged();
}

export function deleteSavingsGoal(args: Record<string, unknown>): void {
  const row = activeGoal(str(args, "id"));
  const saved = accountBalanceCents(row.account_id);
  if (saved !== 0) {
    throw new Error(`替身后端：储蓄目标专属账户余额非零，请先转出 ${row.id}`);
  }
  softDelete(db().savings_goals, row.id, "储蓄目标");
  softDelete(db().accounts, row.account_id, "专属账户");
  emitLedgerChanged();
}

function activeGoal(id: string): SavingsGoal {
  return findActive(db().savings_goals, id, "储蓄目标");
}
