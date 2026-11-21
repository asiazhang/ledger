import { onMounted, ref } from "vue";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import type { DashboardOverview } from "@ledger/types";

export function useDashboardOverview() {
  const overview = ref<DashboardOverview | null>(null);

  const { loading, error, run } = useLoadable(() => api.dashboardOverview());

  async function refresh() {
    overview.value = await run();
  }

  onMounted(() => {
    void refresh();
  });

  return { overview, loading, error, refresh };
}
