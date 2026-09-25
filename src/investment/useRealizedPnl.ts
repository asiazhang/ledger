import { computed, onMounted, ref, watch } from "vue";
import { storeToRefs } from "pinia";
import { api } from "@ledger/api";
import { useInvestmentsSessionStore } from "@/investment/investments-session";
import { useLoadable } from "@ledger/loadable";
import { useReferenceStore } from "@/stores/reference";
import { useInstrumentSearch } from "@/investment/useInstrumentSearch";
import type { RealizedPnlSummary } from "@ledger/types";

/**
 * 已实现盈亏概览：账户/标的筛选 + 汇总数据加载（盈亏 tab 的数据层）。
 *
 * issue #325 起为 Loadable 之上的薄壳（ADR-0040）：loading 置收、错误捕获与文案归一、
 * 错误展示（默认 toast + error 双通道）、竞态裁决全部内化进 Loadable；refresh 为 0 元
 * 发起，闭包内自读当前账户/标的筛选。刷新失败不再静默/产生未处理 rejection（spec 治愈
 * 清单①）：error 置位 + 默认 toast，summary 保持原值不清空成空态。
 * 标的搜索的刻意吞错仍是「刻意静默不收编」的合法形态（词汇表 Loadable 边界），
 * 取数编排自 #1308 起收口 useInstrumentSearch（对 Loadable 的边界不变），本层只做
 * 候选投影（label 拼法）与选中合并。
 */
export function useRealizedPnl() {
  // 账户下拉候选面单点消费（reference.investmentAccountOptions，#1830 收口）
  const { investmentAccountOptions: accountOptions } = storeToRefs(useReferenceStore());
  const session = useInvestmentsSessionStore();

  const summary = ref<RealizedPnlSummary | null>(null);
  const selectedAccountId = ref<string | null>(null);
  const selectedInstrumentId = ref<string | null>(null);

  // 筛选变化翻页归零（issue #1795，词汇表「客户端切片分页」页码生命周期）：
  // 账户/标的筛选任一应用值实际变化即回第一页（同步 flush 与意图应用原子生效，
  // 持仓翻页归零同款；同值重设不触发）。页码住投资页会话状态 store——会话内
  // 保留、冷启动回默认，越界回落由盈亏面板在读出口钳制（回退不归零）。
  watch(
    [selectedAccountId, selectedInstrumentId],
    () => {
      session.setPnlYearPage(1);
      session.setPnlAccountPage(1);
    },
    { flush: "sync" },
  );

  // 标的筛选下拉：取数编排收口 useInstrumentSearch（issue #1308，对外成员名不变），
  // 本层只做候选投影（「代码 · 名称」label 拼法）与选中项合并
  const {
    items: searchedInstruments,
    searching: searchingInstruments,
    search: searchInstruments,
  } = useInstrumentSearch();
  const searchInstrumentOptions = computed(() =>
    searchedInstruments.value.map((i) => ({
      label: `${i.symbol}${i.name ? ` · ${i.name}` : ""}`,
      value: i.id,
    })),
  );
  const selectedInstrumentOption = ref<{ label: string; value: string } | null>(null);

  const pnlInstrumentOptions = computed(() => {
    const opts = [...searchInstrumentOptions.value];
    const sel = selectedInstrumentOption.value;
    if (sel && !opts.some((o) => o.value === sel.value)) {
      opts.push(sel);
    }
    return opts;
  });

  const { loading, error, run } = useLoadable(async () => {
    // 0 元闭包自读当前筛选：发起时点即最新筛选，无需传参
    const filter: Record<string, string | null> = {};
    if (selectedAccountId.value) filter.account_id = selectedAccountId.value;
    if (selectedInstrumentId.value) filter.instrument_id = selectedInstrumentId.value;
    return api.realizedPnlSummary(Object.keys(filter).length > 0 ? filter : undefined);
  });

  async function refresh() {
    const result = await run();
    // 失败回空（error 已置位）：summary 保持原值不清空；迟到前发结果已被 Loadable
    // 竞态裁决作废为空，不会覆写终态
    if (result !== null) summary.value = result;
  }

  function onSelectInstrument(value: string | null) {
    selectedInstrumentId.value = value;
    selectedInstrumentOption.value =
      searchInstrumentOptions.value.find((o) => o.value === value) ?? null;
    void refresh();
  }

  onMounted(() => {
    // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll
    void refresh();
  });

  return {
    loading,
    summary,
    error,
    selectedAccountId,
    selectedInstrumentId,
    accountOptions,
    pnlInstrumentOptions,
    searchingInstruments,
    refresh,
    searchInstruments,
    onSelectInstrument,
  };
}
