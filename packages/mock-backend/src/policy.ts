import type { Policy, PolicyInput, PolicyStats } from "@ledger/types";
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
 * 保单域替身（issue #1974 / spec #1964 / ADR-0143）：保单档案内存态 CRUD +
 * 逐保单统计实时推导，保单页「建保单 → 列表出现 → 记保费 → 统计变化」可用。
 * 从简口径（不追行为等价，与真后端的差异点，发现误导 UI 开发时以真后端为准
 * 修替身）：
 *
 * - **保费 / 流入合计 = 挂单流水 amount_native_cents 直和**（真后端 Measure
 *   矩阵：保费只认 kind=expense、流入只认 kind=income，policy_id 归属、逐笔
 *   可对账、软删流水不计入——替身同边界；金额本位币直记不折算，ADR-0143
 *   决策 1，夹具单币种 CNY）。
 * - **下期扣款日**：真后端由「引用该保单的活跃订阅计划的最早 pending 期次」
 *   三表推导；替身从简，按保单夹具自带的 pending 期次日清单
 *   （state.policy_pending_charges）取最早日，无挂靠即 null——订阅计划夹具
 *   已统一引用真实保单（#1973 并入统一），口径差以真后端为准修替身。
 * - **到期态实时推导**（不持久化，ADR-0051 决策 5）：止日非空且早于替身今日
 *   （MOCK_TODAY 字面量锚）→ 已到期；止日空 = 长期/终身恒未到期。
 * - **native_currency = 账本本位币设置**（db().settings.base_currency）。
 * - **校验只做存在性必需**：保司不存在拒绝（换成新保司才校验在用、维持既有
 *   保司可编辑——真后端同语义）；保单号/险种/日期等真后端校验不模拟
 *   （spec #1964 取向）。
 *
 * 写路径（create / update / delete）完成后统一发 `ledger:changed`（ADR-0143
 * 决策 3：事件驱动失效是保单 store 的重拉触发源）。软删保单不级联软删历史
 * 流水（ADR-0051 决策 5，流水引用原样保留）。
 */

/** 在用保单行，按创建先后排序（与真后端 list_policies 同序）。 */
function activePolicies(): Policy[] {
  return db()
    .policies.filter((p) => !p.is_deleted)
    .sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id));
}

export function listPolicies(): Policy[] {
  return activePolicies();
}

/** 挂单流水直和（kind + policy_id 归属过滤；软删流水与软删保单不计入）。 */
function sumByPolicy(policyId: string, kind: "expense" | "income"): number {
  return db()
    .transactions.filter((t) => !t.is_deleted && t.kind === kind && t.policy_id === policyId)
    .reduce((sum, t) => sum + t.amount_native_cents, 0);
}

export function listPolicyStats(): PolicyStats[] {
  return activePolicies().map((policy) => {
    const pending = db().policy_pending_charges[policy.id] ?? [];
    return {
      policy_id: policy.id,
      native_currency: db().settings.base_currency,
      total_paid_native_cents: sumByPolicy(policy.id, "expense"),
      total_inflow_native_cents: sumByPolicy(policy.id, "income"),
      // 下期扣款 = 挂靠协议 pending 期次最早日（字典序即时间序）；无挂靠 null
      next_charge_date: pending.length > 0 ? [...pending].sort()[0]! : null,
      is_expired: policy.end_date !== null && policy.end_date < MOCK_TODAY,
    };
  });
}

export function createPolicy(args: Record<string, unknown>): string {
  const input = obj<PolicyInput>(args, "input");
  findActive(db().insurers, input.insurer_id, "保司");
  const row: Policy = {
    id: nextId("mock-policy"),
    insurer_id: input.insurer_id,
    policy_number: input.policy_number,
    product_name: input.product_name,
    start_date: input.start_date,
    end_date: input.end_date ?? null,
    coverage_amount_cents: input.coverage_amount_cents ?? null,
    coverage_currency_code: input.coverage_currency_code ?? null,
    note: input.note ?? null,
    is_deleted: false,
    created_at: nowIso(),
    updated_at: nowIso(),
    version: 1,
    device_id: DEVICE_ID,
  };
  db().policies.push(row);
  emitLedgerChanged();
  return row.id;
}

export function updatePolicy(args: Record<string, unknown>): void {
  const row = findActive(db().policies, str(args, "id"), "保单");
  const input = obj<PolicyInput>(args, "input");
  // 换成新保司才校验在用（维持既有保司 = 维持历史引用，真后端同语义）
  if (input.insurer_id !== row.insurer_id) {
    findActive(db().insurers, input.insurer_id, "保司");
  }
  row.insurer_id = input.insurer_id;
  row.policy_number = input.policy_number;
  row.product_name = input.product_name;
  row.start_date = input.start_date;
  row.end_date = input.end_date ?? null;
  row.coverage_amount_cents = input.coverage_amount_cents ?? null;
  row.coverage_currency_code = input.coverage_currency_code ?? null;
  row.note = input.note ?? null;
  touch(row);
  emitLedgerChanged();
}

export function deletePolicy(args: Record<string, unknown>): void {
  softDelete(db().policies, str(args, "id"), "保单");
  emitLedgerChanged();
}
