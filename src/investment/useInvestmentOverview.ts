import { onMounted, ref } from "vue";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import { usePricesChanged } from "@/investment/usePricesChanged";
import type { InvestmentOverview } from "@ledger/types";

export function useInvestmentOverview() {
  const data = ref<InvestmentOverview | null>(null);

  const { loading, error, run } = useLoadable(() => api.investmentOverview());

  async function refresh(): Promise<void> {
    const result = await run();
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
