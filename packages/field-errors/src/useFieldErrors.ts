import { computed, reactive, ref, type ComputedRef, type Ref } from "vue";
import { fieldErrorKind, type FieldErrorKind } from "@ledger/utils/field-error";

export interface FieldJudgment {
  kind: "ok" | FieldErrorKind;
  yuan?: number;
  value?: number;
}

export interface FieldSpec {
  text: Ref<string>;
  judge: (text: string) => FieldJudgment;
  enabled?: () => boolean;
}

export interface FieldView {
  error: ComputedRef<FieldErrorKind | null>;
  value: ComputedRef<number | null>;
  markBlurred: () => void;
}

export interface FieldErrors<K extends string> {
  fields: Record<K, FieldView>;
  hasError: ComputedRef<boolean>;
  markSaveAttempted: () => void;
  reset: () => void;
}

function resolveValue(judgment: FieldJudgment): number | null {
  if (judgment.kind !== "ok") return null;
  return judgment.yuan ?? judgment.value ?? null;
}

export function useFieldErrors<K extends string>(specs: Record<K, FieldSpec>): FieldErrors<K> {
  const keys = Object.keys(specs) as K[];
  const saveAttempted = ref(false);
  const touched = reactive<Record<string, boolean>>({});
  for (const key of keys) touched[key] = false;

  const fields = {} as Record<K, FieldView>;
  for (const key of keys) {
    const spec = specs[key];
    const judgment = computed(() => spec.judge(spec.text.value));
    fields[key] = {
      error: computed<FieldErrorKind | null>(() => {
        if (spec.enabled && !spec.enabled()) return null;
        return fieldErrorKind(judgment.value, {
          touched: touched[key],
          saveAttempted: saveAttempted.value,
        });
      }),
      value: computed(() => resolveValue(judgment.value)),
      markBlurred: () => {
        touched[key] = true;
      },
    };
  }

  return {
    fields,
    hasError: computed(() => keys.some((key) => fields[key].error.value != null)),
    markSaveAttempted: () => {
      saveAttempted.value = true;
    },
    reset: () => {
      saveAttempted.value = false;
      for (const key of keys) touched[key] = false;
    },
  };
}
