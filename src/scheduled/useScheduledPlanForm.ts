import { computed, ref } from "vue";
import type { TreeSelectOption } from "naive-ui";
import { useMessage } from "naive-ui";
import { useReferenceStore } from "@/stores/reference";
import {
  useAccountCurrency,
  useFormShared,
  usePinnedAccountOptions,
} from "@/composables/useFormShared";
import { resolveMerchantRef } from "@/merchants/resolve-merchant";
import { api } from "@ledger/api";
import { errorMessage } from "@ledger/utils/errors";
import { t } from "@ledger/i18n";
import { todayStr } from "@ledger/utils/date";
import type { CreateScheduledInput, RecurrenceType, ScheduledKind } from "@ledger/types";

interface ScheduledPlanSpecificFields {
  total_amount_cents?: number | null;
  total_occurrences?: number | null;
  to_account_id?: string | null;
}

export interface ScheduledPlanCreateSpec {
  kind: ScheduledKind;
  amountCents: number;
  merchantId: string | null;
  specific?: ScheduledPlanSpecificFields;
}

export interface ScheduledPlanSubmitSpec {
  kind: ScheduledKind;
  amountCents: number;
  specific?: ScheduledPlanSpecificFields;
}

type SubmitSuccessCallback = () => void | Promise<void>;

export interface UseScheduledPlanFormOptions {
  onSubmitted?: SubmitSuccessCallback;
}

export function useScheduledPlanForm(options: UseScheduledPlanFormOptions = {}) {
  const { onSubmitted } = options;
  const reference = useReferenceStore();
  const { accountOptions: selectableAccountOptions, currencyOptions } = useFormShared();
  const message = useMessage();

  const note = ref("");
  const accountId = ref<string | null>(null);
  const accountOptions = usePinnedAccountOptions(accountId, selectableAccountOptions);
  const categoryId = ref<string | null>(null);
  const merchantRef = ref<string | null>(null);
  const currencyCode = useAccountCurrency(accountId);
  const recurrenceType = ref<RecurrenceType>("monthly");
  const recurrenceInterval = ref(1);
  const recurrenceDay = ref<number | null>(null);
  const startDate = ref(todayStr());

  function reset() {
    note.value = "";
    accountId.value = null;
    categoryId.value = null;
    merchantRef.value = null;
    recurrenceType.value = "monthly";
    recurrenceInterval.value = 1;
    recurrenceDay.value = null;
    startDate.value = todayStr();
  }

  const categoryTreeOptions = computed(
    () => reference.treeCategoryOptions("expense") as unknown as TreeSelectOption[],
  );

  const merchantOptions = computed<{ label: string; value: string }[]>(() =>
    reference.merchants.map((m) => ({ label: m.name, value: m.id })),
  );

  async function resolveMerchant(editingMerchantId: string | null = null): Promise<string | null> {
    return resolveMerchantRef(merchantRef.value, editingMerchantId);
  }

  function buildCreateInput(spec: ScheduledPlanCreateSpec): CreateScheduledInput {
    return {
      kind: spec.kind,
      account_id: accountId.value!,
      category_id: categoryId.value,
      merchant_id: spec.merchantId,
      amount_cents: spec.amountCents,
      currency_code: currencyCode.value,
      recurrence_type: recurrenceType.value,
      recurrence_interval: recurrenceInterval.value,
      recurrence_day: recurrenceDay.value,
      start_date: startDate.value,
      note: note.value.trim() || null,
      ...spec.specific,
    };
  }

  const CREATE_SUCCESS_KEY: Record<ScheduledKind, string> = {
    subscription: "scheduled.toast.subscriptionCreated",
    installment: "scheduled.toast.installmentCreated",
    scheduled_transfer: "scheduled.toast.transferCreated",
  };

  async function submitCreate(spec: ScheduledPlanSubmitSpec): Promise<void> {
    try {
      const merchantId = spec.kind === "scheduled_transfer" ? null : await resolveMerchant();
      await api.createScheduledTransaction(
        buildCreateInput({
          kind: spec.kind,
          amountCents: spec.amountCents,
          merchantId,
          specific: spec.specific,
        }),
      );
      message.success(t(CREATE_SUCCESS_KEY[spec.kind]));
      reset();
      await onSubmitted?.();
    } catch (e) {
      message.error(t("scheduled.toast.createFailed", { message: errorMessage(e) }));
    }
  }

  return {
    note,
    accountId,
    categoryId,
    merchantRef,
    currencyCode,
    recurrenceType,
    recurrenceInterval,
    recurrenceDay,
    startDate,
    accountOptions,
    currencyOptions,
    categoryTreeOptions,
    merchantOptions,
    reset,
    resolveMerchant,
    buildCreateInput,
    submitCreate,
  };
}
