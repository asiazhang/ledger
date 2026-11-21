import { onMounted, ref } from "vue";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import type { FinancialFreedomOverview } from "@ledger/types";

export function useFinancialFreedom() {
  const data = ref<FinancialFreedomOverview | null>(null);

  const { loading, error, run } = useLoadable(() => api.financialFreedom());

  async function refresh() {
    data.value = await run();
  }

  onMounted(() => {
    void refresh();
  });

  return { data, loading, error, refresh };
}
