import { computed, ref } from "vue";
import { useMessage } from "naive-ui";
import { api } from "@ledger/api";
import { centsToYuan } from "@ledger/money";
import { buildTransferInput } from "@/transaction/transaction-input";
import { judgeAmountText } from "@ledger/utils/field-error";
import { useFieldErrors } from "@ledger/field-errors";
import {
  useAccountCurrency,
  useFormShared,
  usePinnedAccountOptions,
  utcMidnightTimestamp,
} from "@/composables/useFormShared";
import { useMerchantField } from "@/merchants/useMerchantField";
import { t } from "@ledger/i18n";
import type { TransactionModalRow } from "@ledger/types";
import { errorMessage } from "@ledger/utils/errors";

export function useTransferForm(options?: {
  onCreated?: () => void;
  onUpdated?: () => void;
  editing?: () => TransactionModalRow | null;
  createdMessage?: () => string;
}) {
  const { reference, accountOptions: selectableAccountOptions, currencyOptions } = useFormShared();
  const message = useMessage();

  const amountText = ref("");
  const errors = useFieldErrors({
    amount: { text: amountText, judge: judgeAmountText },
  });

  const accountId = ref<string | null>(null);
  const toAccountId = ref<string | null>(null);
  const accountOptions = usePinnedAccountOptions(accountId, selectableAccountOptions);
  const currencyCode = useAccountCurrency(accountId);
  const toAccountOptions = usePinnedAccountOptions(
    toAccountId,
    computed(() =>
      accountId.value == null
        ? selectableAccountOptions.value
        : selectableAccountOptions.value.filter((o) => {
            const acc = reference.accountMap.get(o.value);
            return acc && acc.currency_code === currencyCode.value;
          }),
    ),
  );
  const toEndCurrencyError = computed(() => {
    const from = accountId.value == null ? undefined : reference.accountMap.get(accountId.value);
    const to = toAccountId.value == null ? undefined : reference.accountMap.get(toAccountId.value);
    return from != null && to != null && from.currency_code !== to.currency_code;
  });
  const note = ref("");
  const date = ref(Date.now());

  const editingTx = options?.editing?.() ?? null;
  const { merchantRef, merchantOptions, resolveMerchantId } = useMerchantField(
    editingTx?.merchant_id ?? null,
  );
  if (editingTx) {
    amountText.value = String(
      centsToYuan(editingTx.amount_cents, reference.getCurrency(editingTx.currency_code)),
    );
    accountId.value = editingTx.account_id;
    toAccountId.value = editingTx.to_account_id;
    merchantRef.value = editingTx.merchant_id;
    note.value = editingTx.note ?? "";
    date.value = utcMidnightTimestamp(editingTx.date);
  }

  async function submit() {
    errors.markSaveAttempted();
    if (errors.hasError.value) return;
    if (!accountId.value) {
      message.warning(t("transactions.form.warnSelectFromAccount"));
      return;
    }
    if (!toAccountId.value) {
      message.warning(t("transactions.form.warnSelectToAccount"));
      return;
    }
    if (accountId.value === toAccountId.value) {
      message.warning(t("transactions.form.warnSameAccount"));
      return;
    }
    if (toEndCurrencyError.value) return;
    const amount = errors.fields.amount.value.value;
    if (amount == null) return;
    if (amount <= 0) {
      message.warning(t("transactions.form.warnAmount"));
      return;
    }
    const merchantId = await resolveMerchantId();
    const editing = options?.editing?.() ?? null;
    try {
      const input = buildTransferInput({
        amount,
        currencyCode: currencyCode.value,
        accountId: accountId.value,
        toAccountId: toAccountId.value,
        merchantId,
        note: note.value,
        sourceOrderNo: editing?.source_order_no ?? null,
        date: date.value,
      });
      if (editing) {
        await api.updateTransaction(editing.id, input);
        message.success(t("transactions.form.changesSaved"));
        options?.onUpdated?.();
      } else {
        await api.createTransaction(input);
        message.success(options?.createdMessage?.() ?? t("transactions.form.transferCreated"));
        amountText.value = "";
        errors.reset();
        note.value = "";
        options?.onCreated?.();
      }
    } catch (e) {
      message.error(
        editing
          ? t("transactions.form.saveFailed", { msg: errorMessage(e) })
          : t("transactions.form.createFailed", { msg: errorMessage(e) }),
      );
    }
  }

  function resetForm() {
    amountText.value = "";
    errors.reset();
    accountId.value = null;
    toAccountId.value = null;
    merchantRef.value = null;
    note.value = "";
    date.value = Date.now();
  }

  return {
    amountText,
    markAmountBlurred: errors.fields.amount.markBlurred,
    amountError: errors.fields.amount.error,
    hasFieldError: errors.hasError,
    currencyCode,
    accountId,
    toAccountId,
    toAccountOptions,
    toEndCurrencyError,
    merchantRef,
    merchantOptions,
    note,
    date,
    accountOptions,
    currencyOptions,
    submit,
    resetForm,
  };
}
