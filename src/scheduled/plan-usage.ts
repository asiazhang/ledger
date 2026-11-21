import type { ScheduledTransactionWithExt } from "@ledger/types";
import { isUnfinishedStatus } from "@ledger/scheduled-plan-list";

function isUnfinishedPlan(plan: ScheduledTransactionWithExt): boolean {
  return !plan.core.is_deleted && isUnfinishedStatus(plan.core.status);
}

export function countUnfinishedPlansUsingAccount(
  plans: readonly ScheduledTransactionWithExt[],
  accountId: string,
): number {
  return plans.filter(
    (plan) =>
      isUnfinishedPlan(plan) &&
      (plan.core.account_id === accountId || plan.to_account_id === accountId),
  ).length;
}
