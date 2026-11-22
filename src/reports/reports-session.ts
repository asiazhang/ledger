import { defineStore } from "pinia";
import { ref } from "vue";
import { presetRange, type DateRange } from "@ledger/utils/time-period";

export const MERCHANT_TOP_N_OPTIONS = [5, 10] as const;

export const MERCHANT_TOP_N_DEFAULT: number = 5;

export const useReportsSessionStore = defineStore("reports-session", () => {
  const period = ref<DateRange>(presetRange("year", new Date()));

  const drilledRootId = ref<string | null>(null);

  const merchantTopN = ref<number>(MERCHANT_TOP_N_DEFAULT);

  function setPeriod(range: DateRange) {
    if (range.from === period.value.from && range.to === period.value.to) return;
    period.value = { from: range.from, to: range.to };
    drilledRootId.value = null;
  }

  function setDrilldown(categoryId: string | null) {
    drilledRootId.value = categoryId;
  }

  function setMerchantTopN(n: number) {
    merchantTopN.value = n;
  }

  function resetToDefault() {
    setPeriod(presetRange("year", new Date()));
    setDrilldown(null);
    setMerchantTopN(MERCHANT_TOP_N_DEFAULT);
  }

  return {
    period,
    drilledRootId,
    merchantTopN,
    setPeriod,
    setDrilldown,
    setMerchantTopN,
    resetToDefault,
  };
});
