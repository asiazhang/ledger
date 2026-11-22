import { computed, ref } from "vue";
import { useMessage } from "naive-ui";
import type { TreeSelectOption } from "naive-ui";
import { api } from "@ledger/api";
import { centsToYuan } from "@ledger/money";
import { buildExpenseIncomeInput } from "@/transaction/transaction-input";
import { judgeAmountText } from "@ledger/utils/field-error";
import { useFieldErrors } from "@ledger/field-errors";
import { useReferenceStore } from "@/stores/reference";
import { usePoliciesStore } from "@/policy/policies";
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

export function useCategoryForm(
  kind: "expense" | "income",
  options?: {
    onCreated?: () => void;
    onUpdated?: () => void;
    editing?: () => TransactionModalRow | null;
  },
) {
  const { accountOptions: selectableAccountOptions, currencyOptions } = useFormShared();
  const reference = useReferenceStore();
  const message = useMessage();

  const amountText = ref("");
  const errors = useFieldErrors({
    amount: { text: amountText, judge: judgeAmountText },
  });

  const accountId = ref<string | null>(null);
  const accountOptions = usePinnedAccountOptions(accountId, selectableAccountOptions);
  const currencyCode = useAccountCurrency(accountId);
  const categoryId = ref<string | null>(null);
  const policiesStore = usePoliciesStore();
  const policyId = ref<string | null>(null);
  const note = ref("");
  const date = ref(Date.now());

  const editingTx = options?.editing?.() ?? null;

  const treeOptions = computed<TreeSelectOption[]>(
    () => reference.treeCategoryOptions(kind) as unknown as TreeSelectOption[],
  );

  const { merchantRef, merchantOptions, resolveMerchantId } = useMerchantField(
    editingTx?.merchant_id ?? null,
  );

  const editingPolicyId = editingTx?.policy_id ?? null;
  const policyOptions = computed<{ label: string; value: string }[]>(() => {
    const base = policiesStore.policies.map((p) => ({
      label: `${p.policy_number}（${p.product_name}）`,
      value: p.id,
    }));
    if (editingPolicyId && !policiesStore.policies.some((p) => p.id === editingPolicyId)) {
      base.unshift({
        label: t("transactions.form.policyDeleted"),
        value: editingPolicyId,
      });
    }
    return base;
  });

  if (editingTx) {
    amountText.value = String(
      centsToYuan(editingTx.amount_cents, reference.getCurrency(editingTx.currency_code)),
    );
    accountId.value = editingTx.account_id;
    categoryId.value = editingTx.category_id;
    merchantRef.value = editingTx.merchant_id;
    policyId.value = editingTx.policy_id;
    note.value = editingTx.note ?? "";
    date.value = utcMidnightTimestamp(editingTx.date);
  }

  async function submit() {
    errors.markSaveAttempted();
    if (errors.hasError.value) return;
    if (!accountId.value) {
      message.warning(t("settings.categories.msg.accountRequired"));
      return;
    }
    const amount = errors.fields.amount.value.value;
    if (amount == null) return;
    if (amount <= 0) {
      message.warning(t("settings.categories.msg.amountRequired"));
      return;
    }
    const merchantId = await resolveMerchantId();
    const editing = options?.editing?.() ?? null;
    try {
      const input = buildExpenseIncomeInput({
        kind,
        amount,
        currencyCode: currencyCode.value,
        accountId: accountId.value,
        categoryId: categoryId.value,
        merchantId,
        policyId: policyId.value,
        note: note.value,
        sourceOrderNo: editing?.source_order_no ?? null,
        date: date.value,
      });
      if (editing) {
        await api.updateTransaction(editing.id, input);
        message.success(t("settings.categories.msg.saved"));
        options?.onUpdated?.();
      } else {
        await api.createTransaction(input);
        message.success(
          kind === "expense"
            ? t("settings.categories.msg.expenseRecorded")
            : t("settings.categories.msg.incomeRecorded"),
        );
        amountText.value = "";
        errors.reset();
        note.value = "";
        options?.onCreated?.();
      }
    } catch (e) {
      message.error(
        editing
          ? t("settings.categories.msg.saveFailed", { msg: errorMessage(e) })
          : t("settings.categories.msg.recordFailed", { msg: errorMessage(e) }),
      );
    }
  }

  function resetForm() {
    amountText.value = "";
    errors.reset();
    accountId.value = null;
    categoryId.value = null;
    merchantRef.value = null;
    policyId.value = null;
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
    categoryId,
    merchantRef,
    policyId,
    note,
    date,
    accountOptions,
    currencyOptions,
    treeOptions,
    merchantOptions,
    policyOptions,
    submit,
    resetForm,
  };
}
