import type { ScheduledTransactionWithExt } from "@ledger/types";
import { isUnfinishedStatus } from "@ledger/scheduled-plan-list";

/**
 * 计划引用查询（spec #1952 / ADR-0142 决策 6，issue #1955）：账户归档确认框的
 * 「未结束计划数」提示。
 *
 * 语义边界：归档**不是停用**（ADR-0142 决策 1）——本模块只回答「有多少计划还指着
 * 这个账户」，供确认框陈述事实；不阻断归档、不改任何计划状态。
 *
 * 判据单一出处：状态集合的**定义**在计划清单包
 * （`@ledger/scheduled-plan-list` 的 `isUnfinishedStatus`），本模块只叠加「软删不计入」
 * 一层；调用方不自带 `status === ...` 分支。
 */

/**
 * 计划是否「未结束」（归档提示计数的口径）：
 *
 * - `active`（正常执行）与 `paused`（暂停、暂停后 resume 照常生成期次）都还持有该
 *   账户作为落账账户 → 计入。集合本身不在此定义：与计划清单行操作矩阵同吃
 *   `isUnfinishedStatus`（`@ledger/scheduled-plan-list`；取消限同一集合），
 *   即「未终止」集合只有一份出处。
 * - `cancelled`（取消）与 `completed`（自然完成）不再生成期次、账户不再是它的落账
 *   账户 → 不计入。
 *
 * 软删计划不进提示：读命令 `list_plans` 已滤 `is_deleted=0`，此处再挡一次让判据自足
 * （谓词独立成立，不依赖调用方拿到的是哪条读路径）。
 */
export function isUnfinishedPlan(plan: ScheduledTransactionWithExt): boolean {
  return !plan.core.is_deleted && isUnfinishedStatus(plan.core.status);
}

/**
 * 引用该账户的未结束计划数：付款 / 收款 / 转出 / 转入四类引用取并集——
 * `core.account_id` 是分期与订阅的扣款账户、定时转账的转出账户；`to_account_id`
 * 只对定时转账有意义（其余形态恒 null），即转入账户。同一计划两端都指向该账户时
 * 只计一次（谓词内的或，不是两次计数）。
 */
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
