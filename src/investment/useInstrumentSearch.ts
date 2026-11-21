import { ref, type Ref } from "vue";
import { api } from "@ledger/api";
import { createLatestWins } from "@ledger/latest-wins";
import { SEARCH_DEBOUNCE_MS } from "@/composables/search-debounce";
import type { Instrument } from "@ledger/types";

export interface UseInstrumentSearchReturn {
  items: Ref<Instrument[]>;
  searching: Ref<boolean>;
  search(query: string): void;
}

export function useInstrumentSearch(): UseInstrumentSearchReturn {
  const items = ref<Instrument[]>([]);
  const searching = ref(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const wins = createLatestWins();

  function search(query: string) {
    const myToken = wins.begin();
    clearTimeout(timer);
    timer = setTimeout(async () => {
      if (myToken.isStale()) return;
      if (!query.trim()) {
        items.value = [];
        searching.value = false;
        return;
      }
      searching.value = true;
      try {
        const res = await api.listInstruments({ search: query.trim(), page_size: 50 });
        if (myToken.isStale()) return;
        items.value = res.items;
      } catch {
        if (myToken.isStale()) return;
        items.value = [];
      } finally {
        if (!myToken.isStale()) searching.value = false;
      }
    }, SEARCH_DEBOUNCE_MS);
  }

  return { items, searching, search };
}
