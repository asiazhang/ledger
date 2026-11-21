import { readonly, ref } from "vue";
import type { Ref } from "vue";

export interface UseModalIntentReturn<TIntent> {
  readonly intent: Readonly<Ref<TIntent | null>>;
  readonly seq: Readonly<Ref<number>>;
  open(intent: TIntent): void;
  close(): void;
}

export function useModalIntent<TIntent>(): UseModalIntentReturn<TIntent> {
  const intent = ref(null) as Ref<TIntent | null>;
  const seq = ref(0);

  function open(next: TIntent) {
    intent.value = { ...next };
    seq.value += 1;
  }

  function close() {
    intent.value = null;
  }

  return {
    intent: readonly(intent) as Readonly<Ref<TIntent | null>>,
    seq: readonly(seq),
    open,
    close,
  };
}
