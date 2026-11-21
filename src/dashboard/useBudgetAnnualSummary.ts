import { onMounted, ref } from "vue";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import type { BudgetAnnualSummary } from "@ledger/types";

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
