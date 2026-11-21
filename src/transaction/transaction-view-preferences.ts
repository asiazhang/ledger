import { computed, ref } from "vue";
import { defineStore } from "pinia";
import {
  clearHideInvestmentRelated,
  getSavedHideInvestmentRelated,
  saveHideInvestmentRelated,
} from "@ledger/utils/view-state";

export function parseHideInvestmentRelated(raw: unknown): boolean {
  return raw === true;
}

export const useTransactionViewPreferencesStore = defineStore(
  "transaction-view-preferences",
  () => {
    const hideInvestmentRelated = ref(parseHideInvestmentRelated(getSavedHideInvestmentRelated()));

    function setHideInvestmentRelated(value: boolean) {
      if (value === hideInvestmentRelated.value) return;
      hideInvestmentRelated.value = value;
      if (value) saveHideInvestmentRelated(true);
      else clearHideInvestmentRelated();
    }

    return {
      hideInvestmentRelated: computed(() => hideInvestmentRelated.value),
      setHideInvestmentRelated,
    };
  },
);
