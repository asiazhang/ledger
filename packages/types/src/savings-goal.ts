import type { Syncable } from "./common";

/**
 * 储蓄目标（SavingsGoal）领域类型（spec #1750 / issue #1751 / ADR-0133）：
 * 单列小域——为特定用途设定的资金蓄水目标，创建时账本自动建其专属账户
 * （`other` 类型、1:1 绑定）。蓄水是真实流水、进度是真实余额；金额一律整数分；
 * 目标币种 = 专属账户币种（随进度读数携带，目标域不折算）。
 */

/** 目标生命周期状态：进行中 / 归档（达成是读时派生的纯展示态，不入本闭集）。 */
export type SavingsGoalStatus = "active" | "archived";

/** 储蓄目标实体（读模型，对应后端 `savings_goal::SavingsGoal`）。 */
export interface SavingsGoal extends Syncable {
  id: string;
  /** 目标名称（目标名权威，专属账户名随动只读）。 */
  name: string;
  /** 目标金额（整数分，正数）。 */
  target_amount_cents: number;
  /** 截止日期（可空 = 无截止日；YYYY-MM-DD）。 */
  deadline: string | null;
  /** 生命周期状态（进行中 / 归档）。 */
  status: SavingsGoalStatus;
  /** 手填「计划月存」（可空，整数分；节奏来源闭集二值之一）。 */
  planned_monthly_cents: number | null;
  /** 专属账户绑定（1 目标 : 1 账户）。 */
  account_id: string;
  created_at: string;
}

/** 创建入参（对应后端 `savings_goal::SavingsGoalInput`）：名称、目标金额与
 *  可选截止日期；专属账户由后端同事务自动创建，账户信息不出现在入参。 */
export interface SavingsGoalInput {
  name: string;
  /** 目标金额（整数分；非正数被码化错误拒绝）。 */
  target_amount_cents: number;
  /** 截止日期（可空 = 无截止日；YYYY-MM-DD）。 */
  deadline?: string | null;
}

/** 蓄水进度读数（对应后端 `savings_goal::SavingsGoalProgress`）：
 *  已存 = 专属账户余额（余额缓存），还差为带符号差值，达成为读时派生。 */
export interface SavingsGoalProgress {
  goal: SavingsGoal;
  /** 已存 = 专属账户余额（整数分）。 */
  saved_cents: number;
  /** 还差 = 目标额 − 已存（带符号差值，超额为负）。 */
  remaining_cents: number;
  /** 达成 = 已存 ≥ 目标额（纯展示态）。 */
  achieved: boolean;
  /** 目标币种 = 专属账户币种。 */
  currency_code: string;
}
