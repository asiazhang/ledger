import type {
  CreateScheduledInput,
  ExecuteOccurrenceInput,
  InstallmentPlan,
  ScheduledStatus,
  ScheduledTransaction,
  ScheduledTransactionDetail,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
  ScheduledTransferPlan,
  SubscriptionMonthSpend,
  SubscriptionPlan,
  SubscriptionSpendOverview,
  SubscriptionSpendRow,
  Transaction,
  UpdateStatusInput,
  UpdateSubscriptionInput,
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
  str,
  touch,
} from "./state";

/**
 * 定时/订阅域替身（issue #1973 / spec #1964 / ADR-0143）：计划 + 期次 + 三张
 * 形态扩展行内存态，定时页「建计划 → 期次出现 → 手动执行 → 流水出现」与订阅
 * 花费面板可用。从简口径（不追行为等价，与真后端的差异点，发现误导 UI 开发
 * 时以真后端为准修替身）：
 *
 * - **失效信号**：全部写命令（建计划 / 状态变更 / 订阅编辑 / 期次执行 / 期次
 *   展开）完成后统一发一次 `ledger:changed`（spec #1964「失效信号必发」取向）。
 *   真后端这五条计划写入刻意零信号（命令壳头注，映射单点显式登记）——替身
 *   统一发，多一次重拉无害；期次展开零新增期次时不发（无写入即无信号）。
 * - **订阅花费窗口锚定 MOCK_TODAY（2026-01-15）**：真后端按运行日滚动，替身
 *   不随真实时钟走（确定性纪律，budget.ts 同款先例）；实际花费 = completed
 *   期次关联流水按流水 date 的日历月直和（不摊销、不折算——夹具单币种 CNY，
 *   原币直记即本位币口径）；折算系数同真后端 monthly_coefficient 单点口径
 *   （月 ×1、年 ÷12、周 ×52÷12、日 ×30，间隔 N 再 ÷N），逐计划四舍五入
 *   到分后求和、年成本 = 月成本 × 12。
 * - **校验只做存在性必需**（计划/期次行在用、形态专属数据齐全）：金额正数、
 *   状态机转移合法性（真后端拒绝手动 completed 等）、定时转账禁携商户/保单、
 *   跨币种守卫、订阅编辑商户变更校验等真后端校验不模拟（spec #1964 取向）；
 *   例外：金额哨兵拒绝与「仅订阅可编辑」按票面要求模拟（ADR-0023 决策三）。
 * - **取消级联**：计划置 cancelled 时 pending 期次批量置 cancelled（真后端
 *   同语义，期次详情弹窗可见性依赖）；执行期次后上限计划的自动完成检查
 *   同真后端（分期/有上限转账全期完成 → 计划 completed）。
 *
 * 期次日期推进（advanceDate）与真后端 engine.rs 同款算术：daily/weekly 按
 * 天数；monthly/yearly 按 calendar 月/年推进后日号锚定 recurrence_day（缺省
 * 沿用起始日号），超当月天数收敛到月末（如 31 号在 2 月钳制到 28/29）。
 */

/** 订阅期次展开的预生成窗口大小（engine.rs WINDOW_SIZE 同款）。 */
const OCCURRENCE_WINDOW = 12;

// —— 计划行与扩展行定位（存在性校验：找不到报错） ——

function activePlan(id: string): ScheduledTransaction {
  const row = db().scheduledPlans.find((p) => p.id === id && !p.is_deleted);
  if (!row) throw new Error(`替身后端：定时计划不存在或已删除 ${id}`);
  return row;
}

function installmentExt(planId: string): InstallmentPlan {
  const row = db().installmentPlans.find((p) => p.scheduled_transaction_id === planId);
  if (!row) throw new Error(`替身后端：分期扩展信息不存在 ${planId}`);
  return row;
}

function subscriptionExt(planId: string): SubscriptionPlan {
  const row = db().subscriptionPlans.find((p) => p.scheduled_transaction_id === planId);
  if (!row) throw new Error(`替身后端：订阅扩展信息不存在 ${planId}`);
  return row;
}

function transferExt(planId: string): ScheduledTransferPlan {
  const row = db().scheduledTransferPlans.find((p) => p.scheduled_transaction_id === planId);
  if (!row) throw new Error(`替身后端：定时转账扩展信息不存在 ${planId}`);
  return row;
}

/** 在用期次行（按 id 定位；不存在或已软删报错）。 */
function activeOccurrence(id: string): ScheduledTransactionOccurrence {
  const row = db().scheduledOccurrences.find((o) => o.id === id && !o.is_deleted);
  if (!row) throw new Error(`替身后端：期次不存在或已删除 ${id}`);
  return row;
}

// —— 读：列表与详情 ——

/** 计划行的形态扩展投影（list 与 detail 扩展共用视图）。 */
function extProjection(plan: ScheduledTransaction): ScheduledTransactionWithExt {
  const base = { core: plan };
  switch (plan.kind) {
    case "installment": {
      const ext = installmentExt(plan.id);
      return {
        ...base,
        merchant_id: ext.merchant_id,
        policy_id: null,
        total_amount_cents: ext.total_amount_cents,
        total_occurrences: ext.total_occurrences,
        to_account_id: null,
      };
    }
    case "subscription": {
      const ext = subscriptionExt(plan.id);
      return {
        ...base,
        merchant_id: ext.merchant_id,
        policy_id: ext.policy_id,
        total_amount_cents: null,
        total_occurrences: null,
        to_account_id: null,
      };
    }
    case "scheduled_transfer": {
      const ext = transferExt(plan.id);
      return {
        ...base,
        merchant_id: null,
        policy_id: null,
        total_amount_cents: null,
        total_occurrences: ext.total_occurrences,
        to_account_id: ext.to_account_id,
      };
    }
  }
}

export function listScheduledTransactions(): ScheduledTransactionWithExt[] {
  return db()
    .scheduledPlans.filter((p) => !p.is_deleted)
    .sort((a, b) => b.created_at.localeCompare(a.created_at))
    .map(extProjection);
}

/** 某计划的在用期次（scheduled_date 升序）。 */
function planOccurrences(planId: string): ScheduledTransactionOccurrence[] {
  return db()
    .scheduledOccurrences.filter((o) => o.scheduled_transaction_id === planId && !o.is_deleted)
    .sort((a, b) => a.scheduled_date.localeCompare(b.scheduled_date));
}

export function getScheduledTransactionDetail(
  args: Record<string, unknown>,
): ScheduledTransactionDetail {
  const plan = activePlan(str(args, "id"));
  const extension =
    plan.kind === "installment"
      ? installmentExt(plan.id)
      : plan.kind === "subscription"
        ? subscriptionExt(plan.id)
        : transferExt(plan.id);
  const occurrences = planOccurrences(plan.id);
  const completed = occurrences.filter((o) => o.status === "completed");
  return {
    core: plan,
    extension,
    pending_occurrences: occurrences.filter((o) => o.status === "pending"),
    completed_occurrences: completed.length,
    completed_amount_cents: completed.reduce((sum, o) => sum + o.amount_cents, 0),
    occurrences,
  };
}

// —— 写：建档与期次展开 ——

/** 日期加 offset 期次后的计划日期（engine.rs advance_date 同款：月/年推进后
 *  日号锚定 recurrence_day，超当月天数收敛到月末）。 */
function advanceDate(
  recurrenceType: string,
  recurrenceInterval: number,
  recurrenceDay: number | null,
  from: string,
  offset: number,
): string {
  const [y0, m0, d0] = from.split("-").map(Number);
  const interval = Math.max(recurrenceInterval, 1);
  if (recurrenceType === "daily") {
    return shiftDay(from, interval * offset);
  }
  if (recurrenceType === "weekly") {
    return shiftDay(from, 7 * interval * offset);
  }
  // monthly / yearly：calendar 推进 + 日号锚定（缺省沿用起始日号）
  const totalMonths =
    recurrenceType === "monthly"
      ? y0 * 12 + (m0 - 1) + interval * offset
      : (y0 + interval * offset) * 12 + (m0 - 1);
  const y = Math.floor(totalMonths / 12);
  const m = (totalMonths % 12) + 1;
  const day = recurrenceDay ?? d0;
  const maxDay = daysInMonth(y, m);
  return `${String(y).padStart(4, "0")}-${String(m).padStart(2, "0")}-${String(
    Math.min(day, maxDay),
  ).padStart(2, "0")}`;
}

function daysInMonth(year: number, month: number): number {
  if (month === 2) {
    const leap = (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
    return leap ? 29 : 28;
  }
  return [4, 6, 9, 11].includes(month) ? 30 : 31;
}

/** 日期平移 n 天（daily/weekly 推进；UTC 无时区陷阱）。 */
function shiftDay(date: string, days: number): string {
  const d = new Date(`${date}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + days);
  return d.toISOString().slice(0, 10);
}

export function createScheduledTransaction(args: Record<string, unknown>): string {
  const input = obj<CreateScheduledInput>(args, "input");
  // 存在性校验（spec #1964 取向）：账户必须在用；形态专属数据齐全
  findActive(db().accounts, input.account_id, "账户");
  if (input.kind === "scheduled_transfer" && !input.to_account_id) {
    throw new Error("替身后端：定时转账必须指定目标账户");
  }
  if (
    input.kind === "installment" &&
    (input.total_amount_cents == null || input.total_occurrences == null)
  ) {
    throw new Error("替身后端：分期计划必须指定总金额与总期数");
  }

  const now = nowIso();
  const plan: ScheduledTransaction = {
    id: nextId("mock-sch"),
    kind: input.kind,
    status: "active",
    account_id: input.account_id,
    category_id: input.category_id ?? null,
    amount_cents: input.amount_cents,
    currency_code: input.currency_code,
    recurrence_type: input.recurrence_type,
    recurrence_interval: input.recurrence_interval,
    recurrence_day: input.recurrence_day ?? null,
    start_date: input.start_date,
    note: input.note ?? null,
    created_at: now,
    updated_at: now,
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
  db().scheduledPlans.push(plan);

  // 形态扩展行（scheduled_transfer 不使用商户：真后端行为层拒绝携带，替身忽略）
  switch (input.kind) {
    case "installment":
      db().installmentPlans.push({
        scheduled_transaction_id: plan.id,
        merchant_id: input.merchant_id ?? null,
        total_amount_cents: input.total_amount_cents!,
        total_occurrences: input.total_occurrences!,
      });
      break;
    case "subscription":
      db().subscriptionPlans.push({
        scheduled_transaction_id: plan.id,
        merchant_id: input.merchant_id ?? null,
        policy_id: input.policy_id ?? null,
      });
      break;
    case "scheduled_transfer":
      db().scheduledTransferPlans.push({
        scheduled_transaction_id: plan.id,
        to_account_id: input.to_account_id!,
        total_occurrences: input.total_occurrences ?? null,
      });
      break;
  }

  expandPlanOccurrences(plan.id); // 建档即展开首段期次（真后端同协议）
  emitLedgerChanged();
  return plan.id;
}

/** 期次展开协议（engine.rs expand_occurrences_protocol 同款）：分期/有上限
 *  转账补齐到上限（分期金额 = 总额均摊、末期补尾差）；无上限计划补 12 期
 *  窗口；起始 = 已有最后一期的下一期或 start_date。返回新增期次 id。 */
function expandPlanOccurrences(planId: string): string[] {
  const plan = activePlan(planId);
  if (plan.status !== "active") return [];

  const existing = db().scheduledOccurrences.filter(
    (o) => o.scheduled_transaction_id === planId && !o.is_deleted,
  );
  const total =
    plan.kind === "installment"
      ? installmentExt(planId).total_occurrences
      : plan.kind === "scheduled_transfer"
        ? transferExt(planId).total_occurrences
        : null; // 订阅无上限
  if (total !== null && existing.length >= total) return [];

  const start =
    existing.length > 0
      ? advanceDate(
          plan.recurrence_type,
          plan.recurrence_interval,
          plan.recurrence_day,
          existing.reduce(
            (latest, o) => (o.scheduled_date > latest ? o.scheduled_date : latest),
            existing[0]!.scheduled_date,
          ),
          1,
        )
      : plan.start_date;

  const ids: string[] = [];
  const push = (scheduledDate: string, amountCents: number): void => {
    const occ: ScheduledTransactionOccurrence = {
      id: nextId("mock-sch-occ"),
      scheduled_transaction_id: planId,
      scheduled_date: scheduledDate,
      status: "pending",
      transaction_id: null,
      amount_cents: amountCents,
      created_at: nowIso(),
      updated_at: nowIso(),
      version: 1,
      device_id: DEVICE_ID,
      is_deleted: false,
    };
    db().scheduledOccurrences.push(occ);
    ids.push(occ.id);
  };

  if (plan.kind === "installment" && total !== null) {
    // 分期金额分配：总额 ÷ 总期数取整为基准，尾差补到末期
    const ext = installmentExt(planId);
    const base = Math.trunc(ext.total_amount_cents / ext.total_occurrences);
    const tail = ext.total_amount_cents - base * ext.total_occurrences;
    for (let i = existing.length; i < total; i++) {
      push(
        advanceDate(
          plan.recurrence_type,
          plan.recurrence_interval,
          plan.recurrence_day,
          plan.start_date,
          i,
        ),
        i === total - 1 ? base + tail : base,
      );
    }
  } else {
    for (let i = 0; i < OCCURRENCE_WINDOW; i++) {
      push(
        advanceDate(plan.recurrence_type, plan.recurrence_interval, plan.recurrence_day, start, i),
        plan.amount_cents,
      );
    }
  }
  return ids;
}

export function expandScheduledOccurrences(args: Record<string, unknown>): string[] {
  const ids = expandPlanOccurrences(str(args, "id"));
  if (ids.length > 0) emitLedgerChanged(); // 无新增期次零信号（真后端空窗口同语义）
  return ids;
}

// —— 写：状态变更与订阅编辑 ——

const STATUS_CLOSED_SET: readonly ScheduledStatus[] = [
  "active",
  "paused",
  "cancelled",
  "completed",
];

export function updateScheduledTransactionStatus(args: Record<string, unknown>): void {
  const input = obj<UpdateStatusInput>(args, "input");
  if (!STATUS_CLOSED_SET.includes(input.new_status)) {
    throw new Error(`替身后端：未知计划状态 ${String(input.new_status)}`);
  }
  const plan = activePlan(input.id);
  plan.status = input.new_status;
  touch(plan);
  // 取消级联（engine.rs 同语义）：pending 期次批量置 cancelled
  if (input.new_status === "cancelled") {
    for (const occ of db().scheduledOccurrences) {
      if (
        occ.scheduled_transaction_id === input.id &&
        occ.status === "pending" &&
        !occ.is_deleted
      ) {
        occ.status = "cancelled";
        touch(occ);
      }
    }
  }
  emitLedgerChanged();
}

export function updateScheduledSubscription(args: Record<string, unknown>): void {
  const input = obj<UpdateSubscriptionInput>(args, "input");
  // 金额哨兵（ADR-0023 决策三，Rust de_amount_sentinel 同语义）：请求体 key
  // 一旦出现（含显式 null）即拒绝——改价 = 取消旧计划 + 新建
  const raw = input as unknown as Record<string, unknown>;
  if ("amount_cents" in raw || "total_amount_cents" in raw) {
    throw new Error("替身后端：订阅金额不可编辑：改价 = 取消旧计划 + 新建");
  }
  const plan = activePlan(input.id);
  if (plan.kind !== "subscription") {
    throw new Error("替身后端：仅订阅计划支持编辑非金额字段");
  }
  findActive(db().accounts, input.account_id, "账户");
  // 全量替换非金额字段（真后端同语义：未携带字段由调用方补齐当前值）
  plan.account_id = input.account_id;
  plan.category_id = input.category_id ?? null;
  plan.note = input.note ?? null;
  touch(plan);
  const ext = subscriptionExt(input.id);
  ext.merchant_id = input.merchant_id ?? null;
  emitLedgerChanged();
}

// —— 写：期次执行（真落交易 + 回填 + 自动完成检查） ——

export function executeScheduledOccurrence(args: Record<string, unknown>): string {
  const input = obj<ExecuteOccurrenceInput>(args, "input");
  const occ = activeOccurrence(input.occurrence_id);
  if (occ.status !== "pending" && occ.status !== "failed") {
    throw new Error(`替身后端：期次状态为 ${occ.status}，不能执行`);
  }
  const plan = activePlan(occ.scheduled_transaction_id);
  if (plan.status !== "active") {
    throw new Error("替身后端：关联计划未处于活跃状态");
  }

  // kind 按计划形态（engine.rs 同映射）：分期/订阅 expense（复制商户/保单引用），
  // 转账 transfer（计划扩展行的目标账户）
  const ext = subscriptionOrInstallmentExt(plan);
  const txn: Transaction = {
    id: nextId("mock-txn"),
    kind: plan.kind === "scheduled_transfer" ? "transfer" : "expense",
    amount_cents: occ.amount_cents,
    currency_code: plan.currency_code,
    // 不折算（ADR-0143 决策 1）：本位币金额按原币直记、折算留痕恒空
    amount_native_cents: occ.amount_cents,
    fx_rate_used: null,
    fx_rate_source: null,
    account_id: plan.account_id,
    to_account_id: plan.kind === "scheduled_transfer" ? transferExt(plan.id).to_account_id : null,
    funding_account_id: null,
    category_id: plan.kind === "scheduled_transfer" ? null : plan.category_id,
    merchant_id: plan.kind === "scheduled_transfer" ? null : ext.merchant_id,
    policy_id: plan.kind === "subscription" ? (ext.policy_id ?? null) : null,
    source: null,
    convert: null,
    refund_of_transaction_id: null,
    fundings: [],
    purchases: [],
    source_order_no: null,
    note: plan.note,
    date: occ.scheduled_date,
    created_at: nowIso(),
    updated_at: nowIso(),
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
  db().transactions.push(txn);

  // 期次置 completed 并回填交易 id
  occ.status = "completed";
  occ.transaction_id = txn.id;
  touch(occ);

  checkAndCompletePlan(plan.id);
  emitLedgerChanged();
  return txn.id;
}

/** 分期/订阅的商户/保单引用（转账无此概念，恒空投影）。 */
function subscriptionOrInstallmentExt(plan: ScheduledTransaction): {
  merchant_id: string | null;
  policy_id?: string | null;
} {
  if (plan.kind === "installment") {
    return { merchant_id: installmentExt(plan.id).merchant_id };
  }
  if (plan.kind === "subscription") {
    const ext = subscriptionExt(plan.id);
    return { merchant_id: ext.merchant_id, policy_id: ext.policy_id };
  }
  return { merchant_id: null };
}

/** 上限计划自动完成（engine.rs check_and_complete_plan 同款）：全期次
 *  completed → 计划置 completed（订阅无上限恒不触发）。 */
function checkAndCompletePlan(planId: string): void {
  const plan = activePlan(planId);
  const total =
    plan.kind === "installment"
      ? installmentExt(planId).total_occurrences
      : plan.kind === "scheduled_transfer"
        ? transferExt(planId).total_occurrences
        : null;
  if (total === null) return;
  const completed = db().scheduledOccurrences.filter(
    (o) => o.scheduled_transaction_id === planId && o.status === "completed" && !o.is_deleted,
  ).length;
  if (completed >= total) {
    plan.status = "completed";
    touch(plan);
  }
}

// —— 订阅花费总览（实际口径 + 折算推算，ADR-0023 决策二；头注从简口径） ——

/** 折算月成本系数（spend.rs monthly_coefficient 同款单点口径）。 */
function monthlyCoefficient(recurrenceType: string, recurrenceInterval: number): number {
  const perCycle =
    recurrenceType === "monthly"
      ? 1
      : recurrenceType === "yearly"
        ? 1 / 12
        : recurrenceType === "weekly"
          ? 52 / 12
          : 30; // daily
  return perCycle / Math.max(recurrenceInterval, 1);
}

/** YYYY-MM 递增 n 个月（窗口序列推算，无钳制——月首锚定）。 */
function addMonths(yyyyMm: string, months: number): string {
  const [year, month] = yyyyMm.split("-").map(Number);
  const total = year * 12 + (month - 1) + months;
  return `${Math.floor(total / 12)}-${String((total % 12) + 1).padStart(2, "0")}`;
}

export function subscriptionSpendOverview(): SubscriptionSpendOverview {
  const thisMonth = MOCK_TODAY.slice(0, 7); // "2026-01"
  const thisYear = MOCK_TODAY.slice(0, 4); // "2026"

  // 逐计划逐日历月实际花费：只认 completed 期次关联的在用流水（计划状态不参与
  // 过滤——取消/暂停不影响历史实际花费；不折算，原币直记即本位币口径）
  const byPlan = new Map<string, Map<string, number>>();
  for (const occ of db().scheduledOccurrences) {
    if (occ.status !== "completed" || occ.is_deleted || occ.transaction_id === null) continue;
    const plan = db().scheduledPlans.find(
      (p) => p.id === occ.scheduled_transaction_id && p.kind === "subscription" && !p.is_deleted,
    );
    if (!plan) continue;
    const txn = db().transactions.find((t) => t.id === occ.transaction_id && !t.is_deleted);
    if (!txn) continue;
    const month = txn.date.slice(0, 7);
    const planMap = byPlan.get(plan.id) ?? new Map<string, number>();
    planMap.set(month, (planMap.get(month) ?? 0) + txn.amount_cents);
    byPlan.set(plan.id, planMap);
  }

  // 过去 12 个日历月窗口（含当月，旧→新）；行内花费与顶层汇总同源
  const months: SubscriptionMonthSpend[] = [];
  for (let i = 11; i >= 0; i--) {
    const month = addMonths(thisMonth, -i);
    const native = [...byPlan.values()].reduce((sum, m) => sum + (m.get(month) ?? 0), 0);
    months.push({ month, native_cents: native });
  }
  const thisMonthNative = months.find((m) => m.month === thisMonth)?.native_cents ?? 0;
  const thisYearNative = months
    .filter((m) => m.month.startsWith(thisYear))
    .reduce((sum, m) => sum + m.native_cents, 0);

  // 逐订阅行：全部订阅计划（不过滤状态），商户名左联现名（改名即时生效、软删照常显示）
  const rows: SubscriptionSpendRow[] = db()
    .scheduledPlans.filter((p) => p.kind === "subscription" && !p.is_deleted)
    .sort((a, b) => b.created_at.localeCompare(a.created_at))
    .map((plan) => {
      const planMap = byPlan.get(plan.id);
      const windowSum = (pred: (month: string) => boolean) =>
        months.reduce((sum, m) => sum + (pred(m.month) ? (planMap?.get(m.month) ?? 0) : 0), 0);
      const ext = db().subscriptionPlans.find((p) => p.scheduled_transaction_id === plan.id);
      const merchant = ext?.merchant_id
        ? db().merchants.find((m) => m.id === ext.merchant_id)
        : undefined;
      return {
        plan_id: plan.id,
        note: plan.note,
        merchant_name: merchant?.name ?? null,
        status: plan.status,
        amount_cents: plan.amount_cents,
        currency_code: plan.currency_code,
        this_month_native_cents: windowSum((m) => m === thisMonth),
        this_year_native_cents: windowSum((m) => m.startsWith(thisYear)),
      };
    });

  // 折算推算：只算 active 订阅，逐计划系数折算四舍五入到分后求和（不折算币种）
  const projectedMonth = db()
    .scheduledPlans.filter(
      (p) => p.kind === "subscription" && p.status === "active" && !p.is_deleted,
    )
    .reduce(
      (sum, p) =>
        sum +
        Math.round(p.amount_cents * monthlyCoefficient(p.recurrence_type, p.recurrence_interval)),
      0,
    );

  return {
    native_currency: db().settings.base_currency,
    this_month_native_cents: thisMonthNative,
    this_year_native_cents: thisYearNative,
    months,
    rows,
    projected_month_native_cents: projectedMonth,
    projected_year_native_cents: projectedMonth * 12,
  };
}
