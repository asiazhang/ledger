import { onMounted, ref } from "vue";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import { usePricesChanged } from "@/investment/usePricesChanged";

export function usePriceStaleness() {
  const staleCount = ref(0);
  const thresholdDays = ref(0);

  const { run } = useLoadable(() => api.instrumentPriceStaleness(), { silent: true });

  async function refresh(): Promise<void> {
    const result = await run();
    staleCount.value = result?.stale_count ?? 0;
    thresholdDays.value = result?.threshold_days ?? 0;
  }

  onMounted(() => {
    void refresh();
  });
  usePricesChanged(() => {
    void refresh();
  });

  return { staleCount, thresholdDays, refresh };
}
