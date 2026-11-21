import { nextTick, readonly, ref } from "vue";
import type { Ref } from "vue";

interface RowContextMenuState<TRow> {
  x: number;
  y: number;
  row: TRow;
}

export interface UseRowContextMenuReturn<TRow> {
  readonly state: Readonly<Ref<RowContextMenuState<TRow> | null>>;
  readonly position: Readonly<Ref<{ x: number; y: number }>>;
  open(event: MouseEvent, row: TRow): void;
  close(): void;
  select(key: string | number): void;
}

export function useRowContextMenu<TRow>(
  onSelect: (key: string | number, row: TRow) => void,
): UseRowContextMenuReturn<TRow> {
  const state = ref(null) as Ref<RowContextMenuState<TRow> | null>;
  const position = ref({ x: 0, y: 0 });

  function open(event: MouseEvent, row: TRow) {
    const x = event.clientX;
    const y = event.clientY;
    position.value = { x, y };
    state.value = null;
    void nextTick(() => {
      state.value = { x, y, row };
    });
  }

  function close() {
    state.value = null;
  }

  function select(key: string | number) {
    const current = state.value;
    close();
    if (current) onSelect(key, current.row);
  }

  return {
    state: readonly(state) as Readonly<Ref<RowContextMenuState<TRow> | null>>,
    position: readonly(position) as Readonly<Ref<{ x: number; y: number }>>,
    open,
    close,
    select,
  };
}
