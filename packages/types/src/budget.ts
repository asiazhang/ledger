import type { Syncable } from "./common";

export type BudgetPeriod = "monthly" | "yearly";

/** 预算周期闭集；显示标签在文案资源 budget.period.*（i18n，ADR-0049） */
export const BUDGET_PERIODS: BudgetPeriod[] = ["monthly", "yearly"];

export interface Budget extends Syncable {
  id: string;
  category_id: string;
  period: BudgetPeriod;
  amount_cents: number;
  /** 冻结残留（ADR：永久滚动预算）：仅创建时记录用途，不参与进度计算 */
  start_date: string;
  created_at: string;
}

export interface BudgetInput {
  category_id: string;
  period?: BudgetPeriod;
  amount_cents: number;
  /** 仅记录用途（表单不再提供日期选择器，前端传创建当日） */
  start_date: string;
}

/** 预算编辑入参：金额必填；周期可改（issue #1946，ADR-0029 修订注记），分类不可改
 *  （删旧建新）；period 缺省 = 不改周期（旧调用兼容） */
export interface BudgetUpdateInput {
  amount_cents: number;
  period?: BudgetPeriod;
}

export interface BudgetProgress {
  budget: Budget;
  category_name: string;
  spent_cents: number;
  over_budget: boolean;
}
