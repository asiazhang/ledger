import { onMounted, ref } from "vue";
import { h, type VNode } from "vue";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { pnlSemanticColor } from "@ledger/theme/semantic-colors";
import type { Theme } from "@ledger/theme";
import MwrRateCell from "@/investment/MwrRateCell.vue";
import { useLoadable } from "@ledger/loadable";
import { usePricesChanged } from "@/investment/usePricesChanged";
import type { MoneyWeightedReturnSummary, MwrBasis } from "@ledger/types";

export function renderMwrRateCell(
  rate: number | null | undefined,
  theme: Theme,
  basis: MwrBasis = "annualized",
): string | VNode {
  if (rate === undefined) return "-";
  if (rate === null) return t("investments.pnl.notComputable");
  return h(MwrRateCell, {
    rate,
    color: pnlSemanticColor(rate, theme),
    annualized: basis === "annualized",
  });
}

export function useMoneyWeightedReturn() {
  const summary = ref<MoneyWeightedReturnSummary | null>(null);

  const { loading, error, run } = useLoadable(async () => api.moneyWeightedReturnSummary());

  async function refresh() {
    const result = await run();
    if (result !== null) summary.value = result;
  }

  usePricesChanged(() => {
    void refresh();
  });

  onMounted(() => {
    void refresh();
  });

  function instrumentRate(accountId: string, instrumentId: string): number | null | undefined {
    return summary.value?.by_instrument.find(
      (r) => r.account_id === accountId && r.instrument_id === instrumentId,
    )?.rate;
  }

  function instrumentMwr(
    accountId: string,
    instrumentId: string,
  ): { basis: MwrBasis; rate: number | null } | undefined {
    const row = summary.value?.by_instrument.find(
      (r) => r.account_id === accountId && r.instrument_id === instrumentId,
    );
    return row === undefined ? undefined : { basis: row.basis, rate: row.rate };
  }

  return { loading, error, summary, refresh, instrumentRate, instrumentMwr };
}
