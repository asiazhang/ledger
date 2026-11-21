import { onMounted, ref, watch } from "vue";
import { storeToRefs } from "pinia";
import { api } from "@ledger/api";
import { useInvestmentsSessionStore } from "@/investment/investments-session";
import { useLoadable } from "@ledger/loadable";
import { useReferenceStore } from "@/stores/reference";
import { useInstrumentOptions } from "@/investment/useInstrumentOptions";
import { usePinnedAccountOptions } from "@/composables/useFormShared";
import type { RealizedPnlSummary } from "@ledger/types";

export function useRealizedPnl() {
  const { selectableInvestmentAccountOptions } = storeToRefs(useReferenceStore());
  const session = useInvestmentsSessionStore();

  const summary = ref<RealizedPnlSummary | null>(null);
  const selectedAccountId = ref<string | null>(null);
  const accountOptions = usePinnedAccountOptions(
    selectedAccountId,
    selectableInvestmentAccountOptions,
  );
  const selectedInstrumentId = ref<string | null>(null);

  watch(
    [selectedAccountId, selectedInstrumentId],
    () => {
      session.setPnlYearPage(1);
      session.setPnlAccountPage(1);
    },
    { flush: "sync" },
  );

  const {
    options: pnlInstrumentOptions,
    searching: searchingInstruments,
    search: searchInstruments,
    pin: pinInstrumentOption,
    findInstrument,
  } = useInstrumentOptions("tail");

  const { loading, error, run } = useLoadable(async () => {
    const filter: Record<string, string | null> = {};
    if (selectedAccountId.value) filter.account_id = selectedAccountId.value;
    if (selectedInstrumentId.value) filter.instrument_id = selectedInstrumentId.value;
    return api.realizedPnlSummary(Object.keys(filter).length > 0 ? filter : undefined);
  });

  async function refresh() {
    const result = await run();
    if (result !== null) summary.value = result;
  }

  function onSelectInstrument(value: string | null) {
    selectedInstrumentId.value = value;
    pinInstrumentOption(value === null ? null : (findInstrument(value) ?? null));
    void refresh();
  }

  onMounted(() => {
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
