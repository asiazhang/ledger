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

function activePolicies(): Policy[] {
  return db()
    .policies.filter((p) => !p.is_deleted)
    .sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id));
}

export function listPolicies(): Policy[] {
  return activePolicies();
}

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
