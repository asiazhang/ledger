import { onMounted, ref } from "vue";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import { usePricesChanged } from "@/investment/usePricesChanged";
import type { InvestmentOverview } from "@ledger/types";

/**
 * 投资概览数据层（spec #1532 / issue #1536）：消费后端 `investment_overview`
 * 单一读命令——两腿折算、缺价跳过与未计入计数全在后端投资域完成，前端只持
 * 结果与装配渲染（ADR-0130：全页折本位币单值；前端不出现第二份口径表达式，
 * 也不做任何分组或折算）。
 *
 * 刷新时机两处：挂载一次（进入/重挂载即取数），以及价格失效信号
 * `ledger:prices-changed` 后重拉（ADR-0031）——同步标的信息、录入报价或后台
 * 补全实际写价后数字自动翻新，不需要调用方记得手动刷新（与持仓页签、标的页
 * 列表、组合走势、价格过期提示同批消费方）。
 *
 * loading 置收、错误捕获与文案归一、竞态裁决全部内化进 Loadable（ADR-0040）；
 * 缺折算汇率等命令报错不向上抛：转入 error 兜底状态，由视图渲染卡内警告并可
 * 重试（重试即再次 refresh，不显示半截数字）。
 */
export function useInvestmentOverview() {
  const data = ref<InvestmentOverview | null>(null);

  const { loading, error, run } = useLoadable(() => api.investmentOverview());

  async function refresh(): Promise<void> {
    const result = await run();
    // 失败（result 为 null）时 error 已置位：保留旧值不清空成空态，由视图切到
    // 警告态；迟到前发结果已被 Loadable 竞态裁决作废
    if (result !== null) data.value = result;
  }

  onMounted(() => {
    void refresh();
  });
  usePricesChanged(() => {
    void refresh();
  });

  return { data, loading, error, refresh };
}
