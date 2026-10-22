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

/**
 * 储蓄目标域替身（issue #1975 / spec #1964 / ADR-0143）：目标 + 专属账户内存态
 * 读写闭环，目标页「建目标 → 专属账户出现 → 蓄水（往专属账户记账）→ 进度
 * 变化」可用。从简口径（不追行为等价，与真后端的差异点，发现误导 UI 开发时
 * 以真后端为准修替身）：
 *
 * - **已存 = 专属账户余额直和**：复用 ledger.ts 唯一余额口径（期初 + Σ 流水），
 *   不模拟余额缓存（真后端经缓存读，ADR-0067）；往专属账户记一笔交易（既有
 *   create_transaction）即改变已存——蓄水闭环的机制。
 * - **节奏来源只认手填**（pace_source = manual）：真后端关联计划折算（指向
 *   专属账户的在用定时转账计划，pace.rs）优先——定时域替身（spec #1964 批次 7）
 *   落地后再接线，届时计划来源替换手填优先级；未手填即节奏为零，推算字段
 *   整体缺席、不虚构时点（真后端同语义）。
 * - **推算窗口锚定 MOCK_TODAY（2026-01-15）**：真后端按运行日推算，替身不随
 *   真实时钟走（确定性纪律）；算术与真后端 pace.rs 同款——ETA/所需月存均向上
 *   取整，月差 = 日历年月序号差（截止当月按 1 个整月计），达成或截止日已过
 *   不推算。
 * - **校验只做存在性必需**（目标行在用、名称/目标额缺失报错）：目标额正数、
 *   手填月存正数等真后端校验不模拟（spec #1964 取向，budget.ts 同款先例）。
 *
 * 写路径（create / update / archive / unarchive / delete）完成后统一发
 * `ledger:changed`（ADR-0143 决策 3）——create 同步建账户 + delete 级联删账户
 * 均为一次信号（真后端同事务单次失效广播的替身投影）。
 */

/** 双向推算投影（真后端 pace.rs::project 同款算术，纯参数无状态）。 */
interface Projection {
  eta_months: number | null;
  eta_month: string | null;
  required_monthly_cents: number | null;
  pace_delta_cents: number | null;
}

/** 整数上取整除法（两除数均正，pace.rs div_ceil 同款）。 */
function divCeil(a: number, b: number): number {
  return Math.ceil(a / b);
}

/** 月序号（年 × 12 + 月）：月差 = 序号差（pace.rs month_diff 同款）。 */
function monthIndex(date: string): number {
  const [year, month] = date.split("-");
  return Number(year) * 12 + Number(month);
}

/** 年月加月数 → YYYY-MM（预计达成年月：今天所在月 + ETA 月数）。 */
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
  if (remainingCents <= 0) return projection; // 达成 / 超额：不再推算时点
  // 无截止正推：还差 N 个月 + 预计年月（节奏为零不虚构）
  if (deadline === null && pace !== null && pace > 0) {
    const months = divCeil(remainingCents, pace);
    projection.eta_months = months;
    projection.eta_month = addMonths(MOCK_TODAY.slice(0, 7), months);
    return projection;
  }
  // 有截止反推：每月需存 + 落后 / 超前差值（截止日已过不虚构月存）
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
    // 节奏来源只认手填（见头注）；计划来源待定时域替身落地后接线
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
  // 四字段全量替换（真后端同语义：全量携带，可空字段 null = 清除）
  row.name = name;
  row.target_amount_cents = input.target_amount_cents;
  row.deadline = input.deadline ?? null;
  row.planned_monthly_cents = input.planned_monthly_cents ?? null;
  // 改名联动（目标名权威、账户名随动只读）：名称未变则账户零写入
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
  // 同「事务」自动建专属账户（真后端同事务同款：other 类型、币种取账本本位币、
  // 账户名 = 目标名）——不存在「有账户没目标」的中间态（两行写入后统一一次信号）
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
    planned_monthly_cents: null, // 创建入口不携带（编辑才引入，真后端同语义）
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
  // 余额守卫：专属账户余额非零拒绝、引导先转出（真后端同守卫，码化错误的替身简化）
  const saved = accountBalanceCents(row.account_id);
  if (saved !== 0) {
    throw new Error(`替身后端：储蓄目标专属账户余额非零，请先转出 ${row.id}`);
  }
  // 余额为零：目标软删 + 级联软删专属账户（同一「事务」）；流水保留可查
  softDelete(db().savings_goals, row.id, "储蓄目标");
  softDelete(db().accounts, row.account_id, "专属账户");
  emitLedgerChanged();
}

/** 在用目标行（存在性校验：不存在或已软删报错）。 */
function activeGoal(id: string): SavingsGoal {
  return findActive(db().savings_goals, id, "储蓄目标");
}
