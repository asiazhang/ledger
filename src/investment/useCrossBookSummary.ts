import { onMounted, ref } from "vue";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import type { CrossBookInvestmentSummary } from "@ledger/types";

/**
 * 跨账本投资汇总数据层（issue #1196 / ADR-0114）：消费后端
 * `cross_book_investment_summary` 聚合命令——多库只读聚合、逐本状态与折算标注
 * 全部在后端完成，视图只做装配渲染（仪表盘同款纪律）。缺汇率等命令报错转入
 * error 兜底状态（带后端本地化错误信息），由视图显示提示而非空数字。
 */
export function useCrossBookSummary() {
  const summary = ref<CrossBookInvestmentSummary | null>(null);

  const { loading, error, run } = useLoadable(() => api.crossBookInvestmentSummary());

  async function refresh() {
    summary.value = await run();
  }

  onMounted(() => {
    void refresh();
  });

  return { summary, loading, error, refresh };
}
