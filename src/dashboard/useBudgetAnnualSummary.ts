import { onMounted, ref } from "vue";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import type { BudgetAnnualSummary } from "@ledger/types";

/**
 * dashboard「本年度预算」卡数据层（issue #1948 的年度汇总口径，本卡为首页聚合入口）：
 * 消费后端 `budget_annual_summary` 聚合命令——年度预算总额（月预算 ×12 年化）与
 * 本年已支全在后端算好，前端不出现第二份年化表达式，视图只做装配渲染与超标标签派生。
 *
 * 口径单源：预算页年度汇总卡（BudgetView）消费同一命令，两处数字必然一致——
 * 首页只加「一眼可见」，不重算、不另拍口径。
 *
 * 取数时机、loading 收口、错误归一与重试语义与 useFinancialFreedom 同款（ADR-0040
 * 薄壳）：挂载首跑，导航返回重挂载即最新；命令报错转 error 兜底状态（带后端中文
 * 错误信息），由视图显示卡内警告并可重试。
 */
export function useBudgetAnnualSummary() {
  const data = ref<BudgetAnnualSummary | null>(null);

  const { loading, error, run } = useLoadable(() => api.budgetAnnualSummary());

  async function refresh() {
    data.value = await run();
  }

  onMounted(() => {
    void refresh();
  });

  return { data, loading, error, refresh };
}
