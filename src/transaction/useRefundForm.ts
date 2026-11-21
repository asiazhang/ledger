import { computed, ref } from "vue";
import { useMessage } from "naive-ui";
import { api } from "@ledger/api";
import { centsToYuan, formatAmount } from "@ledger/money";
import { buildRefundInput } from "@/transaction/transaction-input";
import { judgeAmountText } from "@ledger/utils/field-error";
import { useFieldErrors } from "@ledger/field-errors";
import {
  useEntityCurrency,
  useFormShared,
  usePinnedAccountOptions,
} from "@/composables/useFormShared";
import { t } from "@ledger/i18n";
import type { Transaction } from "@ledger/types";
import { errorMessage } from "@ledger/utils/errors";

export function useRefundForm(options?: {
  onCreated?: () => void;
  fixedTarget?: () => Transaction | null;
}) {
  const { reference, accountOptions: selectableAccountOptions, currencyOptions } = useFormShared();
  const message = useMessage();

  const amountText = ref("");
  const errors = useFieldErrors({
    amount: { text: amountText, judge: judgeAmountText },
  });

  const accountId = ref<string | null>(null);
  const accountOptions = usePinnedAccountOptions(accountId, selectableAccountOptions);
  const refundTargetId = ref<string | null>(null);
  const note = ref("");
  const date = ref(Date.now());

  const fixedTarget = options?.fixedTarget;
  const fixedTx = fixedTarget?.() ?? null;
  if (fixedTx) {
    amountText.value = String(
      centsToYuan(fixedTx.amount_cents, reference.getCurrency(fixedTx.currency_code)),
    );
    accountId.value = fixedTx.account_id;
  }

  const transactions = ref<Transaction[]>([]);

  const expenseTransactions = computed(() =>
    transactions.value.filter((t) => t.kind === "expense"),
  );

  const refundTargetOptions = computed(() =>
    expenseTransactions.value.map((t) => {
      const cat = reference.categoryPath(t.category_id) || "-";
      const cur = reference.getCurrency(t.currency_code);
      const amt = formatAmount(t.amount_native_cents, cur);
      const noteStr = t.note ? ` · ${t.note}` : "";
      return {
        label: `${t.date}  ${amt}  ${cat}${noteStr}`,
        value: t.id,
      };
    }),
  );

  const refundTarget = computed<Transaction | null>(() => {
    const fixed = fixedTarget?.();
    if (fixed) return fixed;
    return refundTargetId.value == null
      ? null
      : (expenseTransactions.value.find((t) => t.id === refundTargetId.value) ?? null);
  });

  const currencyCode = useEntityCurrency(refundTarget);

  async function loadTransactions() {
    try {
      transactions.value = (await api.listTransactions()).items;
    } catch {}
  }

  async function submit() {
    errors.markSaveAttempted();
    if (errors.hasError.value) return;
    const targetId = fixedTarget?.()?.id ?? refundTargetId.value;
    if (!targetId) {
      message.warning(t("transactions.refund.warnNoTarget"));
      return;
    }
    const amount = errors.fields.amount.value.value;
    if (amount == null) return;
    if (amount <= 0) {
      message.warning(t("transactions.refund.warnNoAmount"));
      return;
    }
    try {
      const input = buildRefundInput({
        amount,
        currencyCode: currencyCode.value,
        accountId: refundTarget.value?.account_id ?? accountId.value,
        refundOfTransactionId: targetId,
        note: note.value,
        date: date.value,
      });
      await api.createTransaction(input);
      message.success(t("transactions.refund.created"));
      if (!fixedTarget) await loadTransactions();
      amountText.value = "";
      errors.reset();
      note.value = "";
      refundTargetId.value = null;
      options?.onCreated?.();
    } catch (e) {
      message.error(t("transactions.refund.failed", { msg: errorMessage(e) }));
    }
  }

  function resetForm() {
    amountText.value = "";
    errors.reset();
    accountId.value = null;
    refundTargetId.value = null;
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
    refundTargetId,
    note,
    date,
    accountOptions,
    currencyOptions,
    expenseTransactions,
    refundTargetOptions,
    refundTarget,
    submit,
    resetForm,
  };
}
