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

/**
 * 编辑入参（对应后端 `savings_goal::SavingsGoalUpdateInput`，issue #1752）：
 * 四字段全量替换——名称（目标名权威，专属账户名随动只读）、目标金额
 *（非正数被码化错误拒绝，与创建同校验）、截止日期与手填「计划月存」
 *（可空 = 清除；携带时必须为正数）。账户信息不出现在入参。
 */
export interface SavingsGoalUpdateInput {
  name: string;
  target_amount_cents: number;
  deadline: string | null;
  planned_monthly_cents: number | null;
}
/** 节奏来源闭集（issue #1753）：关联计划折算（plan）/ 手填「计划月存」（manual）；
 *  节奏为零时进度读数的节奏字段整体缺席，不外发第三个枚举值。 */
export type SavingsGoalPaceSource = "plan" | "manual";

/** 蓄水进度读数（对应后端 `savings_goal::SavingsGoalProgress`）：
 *  已存 = 专属账户余额（余额缓存），还差为带符号差值，达成为读时派生；
 *  双向推算（issue #1753）随行携带——节奏 + 来源 + 无截止 ETA（还差 N 个月 /
 *  预计年月）+ 有截止所需月存与落后 / 超前差值；达成或节奏为零时相应字段
 *  为 null（界面给设置引导而非虚构时点），无百分比口径。 */
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
  /** 当前节奏（月存，整数分；null = 节奏为零——无在用计划且未手填）。 */
  pace_monthly_cents: number | null;
  /** 节奏来源（闭集 plan / manual；节奏为零为 null）。 */
  pace_source: SavingsGoalPaceSource | null;
  /** 无截止正推——还差 N 个月（上取整）；未达成且有节奏才有值，否则 null。 */
  eta_months: number | null;
  /** 无截止正推——预计达成年月（YYYY-MM）；同上缺席为 null。 */
  eta_month: string | null;
  /** 有截止反推——每月需存（上取整）；有截止、未达成且截止日未过才有值。 */
  required_monthly_cents: number | null;
  /** 落后 / 超前差值 = 当前节奏 − 所需月存（正 = 超前、负 = 落后）；任一侧缺席为 null。 */
  pace_delta_cents: number | null;
}
