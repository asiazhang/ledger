import type {
  ScheduledTransaction,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
} from "@ledger/types";

/**
 * 计划实体工厂三形态 + 期次工厂（issue #822 收敛；#1322 起住址上收共享测试支持包）：
 * 计划按三形态各设一厂，形态不变量由工厂保证而非调用方自觉（分期每期金额 =
 * 总额÷期数向下取整、转账对方账户随参）。期次默认额与订阅厂默认额同源（1500 分），
 * 默认计划 + 默认期次组合自洽。
 *
 * 住址说明：四厂原住壳侧 src/__tests__/factories.ts；#1322 抽出
 * @ledger/scheduled-plan-list 时其包内测试跟随被测包，而结构守门规则 4
 * （scripts/check-test-stubs.ts）禁止测试文件本地定义名单工厂——共享工厂层出口
 * 随测试面上收本包，壳侧 factories.ts 经再导出保持既有 import 面不变。
 * 消费形态：包名深导入 `@ledger/test-support/plan-factories`（exports `./*` 通配）。
 */

/** 订阅计划工厂：core.kind 固定 subscription；商户为形态专属参数（可携，默认无）。 */
export function makeSubscriptionPlan(
  partial: Partial<ScheduledTransaction> & { id: string },
  merchant_id: string | null = null,
): ScheduledTransactionWithExt {
  const core: ScheduledTransaction = {
    kind: "subscription",
    status: "active",
    account_id: "acc-1",
    category_id: "cat-1",
    amount_cents: 1500,
    currency_code: "CNY",
    recurrence_type: "monthly",
    recurrence_interval: 1,
    recurrence_day: null,
    start_date: "2026-01-01",
    note: "视频会员",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    ...partial,
  };
  return {
    core,
    merchant_id,
    policy_id: null,
    total_amount_cents: null,
    total_occurrences: null,
    to_account_id: null,
  };
}

/** 分期计划工厂：core.kind 固定 installment；总额与期数必传，每期金额厂内派生保证不变量。 */
export function makeInstallmentPlan(
  partial: Partial<ScheduledTransaction> & { id: string },
  total_amount_cents: number,
  total_occurrences: number,
  merchant_id: string | null = null,
): ScheduledTransactionWithExt {
  const core: ScheduledTransaction = {
    kind: "installment",
    status: "active",
    account_id: "acc-1",
    category_id: "cat-1",
    amount_cents: Math.floor(total_amount_cents / total_occurrences),
    currency_code: "CNY",
    recurrence_type: "monthly",
    recurrence_interval: 1,
    recurrence_day: null,
    start_date: "2026-01-01",
    note: "手机分期",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    ...partial,
  };
  return {
    core,
    merchant_id,
    policy_id: null,
    total_amount_cents,
    total_occurrences,
    to_account_id: null,
  };
}

/** 定时转账计划工厂：core.kind 固定 scheduled_transfer；对方账户必传，期数可选（一次性为 null）。 */
export function makeTransferPlan(
  partial: Partial<ScheduledTransaction> & { id: string },
  to_account_id: string | null,
  total_occurrences: number | null = null,
): ScheduledTransactionWithExt {
  const core: ScheduledTransaction = {
    kind: "scheduled_transfer",
    status: "active",
    account_id: "acc-cny1",
    category_id: null,
    amount_cents: 50000,
    currency_code: "CNY",
    recurrence_type: "monthly",
    recurrence_interval: 1,
    recurrence_day: null,
    start_date: "2026-01-01",
    note: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    ...partial,
  };
  return {
    core,
    merchant_id: null,
    policy_id: null,
    total_amount_cents: null,
    total_occurrences,
    to_account_id,
  };
}

/** 期次工厂：默认挂在 plan-1（与各厂默认用例 id 惯例衔接）、pending、1500 分。 */
export function makeOccurrence(
  partial: Partial<ScheduledTransactionOccurrence> & { id: string },
): ScheduledTransactionOccurrence {
  return {
    scheduled_transaction_id: "plan-1",
    scheduled_date: "2026-03-01",
    status: "pending",
    transaction_id: null,
    amount_cents: 1500,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    ...partial,
  };
}
