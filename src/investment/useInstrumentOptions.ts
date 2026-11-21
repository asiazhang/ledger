import { computed, ref, type ComputedRef, type Ref } from "vue";
import { useInstrumentSearch } from "@/investment/useInstrumentSearch";
import type { Instrument } from "@ledger/types";

interface InstrumentOption {
  label: string;
  value: string;
}

interface InstrumentRef {
  id: string;
  symbol: string;
  name: string | null;
}

function instrumentOptionLabel(symbol: string, name: string | null): string {
  return name ? `${symbol} · ${name}` : symbol;
}

function toInstrumentOption(i: InstrumentRef): InstrumentOption {
  return { label: instrumentOptionLabel(i.symbol, i.name), value: i.id };
}

export interface UseInstrumentOptionsReturn {
  options: ComputedRef<InstrumentOption[]>;
  searching: Ref<boolean>;
  search(query: string): void;
  pin(instrument: InstrumentRef | null): void;
  findInstrument(id: string): Instrument | undefined;
}

export function useInstrumentOptions(pinnedAt: "head" | "tail"): UseInstrumentOptionsReturn {
  const { items, searching, search } = useInstrumentSearch();
  const pinned = ref<InstrumentRef | null>(null);

  const options = computed<InstrumentOption[]>(() => {
    const projected = items.value.map(toInstrumentOption);
    const pinnedOption = pinned.value ? toInstrumentOption(pinned.value) : null;
    if (!pinnedOption || projected.some((o) => o.value === pinnedOption.value)) {
      return projected;
    }
    return pinnedAt === "head" ? [pinnedOption, ...projected] : [...projected, pinnedOption];
  });

  function pin(instrument: InstrumentRef | null) {
    pinned.value = instrument;
  }

  function findInstrument(id: string): Instrument | undefined {
    return items.value.find((i) => i.id === id);
  }

  return { options, searching, search, pin, findInstrument };
}
