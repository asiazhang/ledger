import { onMounted, ref } from "vue";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import type { CrossBookInvestmentSummary } from "@ledger/types";

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
