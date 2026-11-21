import { computed, ref } from "vue";
import { storeToRefs } from "pinia";
import { useMessage } from "naive-ui";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import {
  centsToYuan,
  priceToYuan,
  yuanToCents,
  PRICE_UNITS_PER_FEN,
  PRICE_UNITS_PER_YUAN,
} from "@ledger/money";
import { judgeQuantityText, judgePriceText } from "@ledger/utils/field-error";
import { useFieldErrors } from "@ledger/field-errors";
import {
  useAccountCurrency,
  useFormShared,
  usePinnedAccountOptions,
  utcMidnightTimestamp,
} from "@/composables/useFormShared";
import { useInstrumentOptions } from "@/investment/useInstrumentOptions";
import { buildTradeInput } from "@/transaction/transaction-input";
import { errorMessage } from "@ledger/utils/errors";
import { useSavingsGoalsStore } from "@/savings-goal/savingsGoals";
import type { TransactionModalRow, TransactionTrade } from "@ledger/types";

export function useInvestmentForm(
  kind: "buy" | "sell",
  options?: {
    onCreated?: () => void;
    onUpdated?: () => void;
    editing?: () => TransactionModalRow | null;
    trade?: () => TransactionTrade | null;
  },
) {
  const { reference, currencyOptions } = useFormShared();
  const message = useMessage();

  const accountId = ref<string | null>(null);
  const fundingAccountId = ref<string | null>(null);
  const instrumentId = ref<string | null>(null);
  const amount = ref<number | null>(null);
  const quantityText = ref("");
  const priceText = ref("");
  const fee = ref<number | null>(null);
  const note = ref("");
  const date = ref(Date.now());
  const currencyCode = useAccountCurrency(accountId);

  const {
    options: instrumentOptions,
    searching: searchingInstruments,
    search: searchInstruments,
    pin: pinInstrumentOption,
    findInstrument,
  } = useInstrumentOptions("head");

  const { selectableInvestmentAccountOptions } = storeToRefs(reference);
  const investmentAccountOptions = usePinnedAccountOptions(
    accountId,
    selectableInvestmentAccountOptions,
  );

  const savingsGoalsStore = useSavingsGoalsStore();
  const fundingAccountOptions = usePinnedAccountOptions(
    fundingAccountId,
    computed(() =>
      reference.fundingCandidateAccounts
        .filter(
          (a) =>
            a.currency_code === currencyCode.value && !savingsGoalsStore.goalAccountIds.has(a.id),
        )
        .map((a) => ({ label: a.name, value: a.id })),
    ),
  );

  const editingTx = options?.editing?.() ?? null;
  const editingTrade = options?.trade?.() ?? null;
  const seededInstrumentIsFund = editingTrade?.instrument_type === "fund";
  if (editingTrade) {
    pinInstrumentOption({
      id: editingTrade.instrument_id,
      symbol: editingTrade.symbol,
      name: editingTrade.instrument_name,
    });
  }
  if (editingTx && editingTrade) {
    accountId.value = editingTx.account_id;
    instrumentId.value = editingTrade.instrument_id;
    fundingAccountId.value = editingTx.funding_account_id ?? null;
    quantityText.value = String(editingTrade.quantity);
    if (seededInstrumentIsFund) {
      amount.value = centsToYuan(
        editingTx.amount_cents,
        reference.getCurrency(editingTx.currency_code),
      );
    } else {
      priceText.value = String(priceToYuan(editingTrade.price_cents));
    }
    fee.value =
      editingTrade.fee_cents != null
        ? centsToYuan(editingTrade.fee_cents, reference.getCurrency(editingTx.currency_code))
        : null;
    note.value = editingTx.note ?? "";
    date.value = utcMidnightTimestamp(editingTx.date);
  }

  const isFundInstrument = computed(() => {
    const id = instrumentId.value;
    if (id == null) return false;
    const found = findInstrument(id);
    if (found) return found.type === "fund";
    return id === editingTrade?.instrument_id && seededInstrumentIsFund;
  });

  const errors = useFieldErrors({
    quantity: { text: quantityText, judge: judgeQuantityText },
    price: { text: priceText, judge: judgePriceText, enabled: () => !isFundInstrument.value },
  });
  const {
    error: quantityError,
    value: quantityValue,
    markBlurred: markQuantityBlurred,
  } = errors.fields.quantity;
  const {
    error: priceError,
    value: priceValue,
    markBlurred: markPriceBlurred,
  } = errors.fields.price;
  const hasFieldError = errors.hasError;

  const derivedPrice = computed<number | null>(() => {
    if (!isFundInstrument.value) return null;
    const qty = quantityValue.value;
    if (amount.value == null || qty == null || qty <= 0) return null;
    const amountCents = yuanToCents(amount.value);
    if (amountCents == null) return null;
    const feeCents = fee.value == null ? 0 : (yuanToCents(fee.value) ?? 0);
    const baseCents = kind === "buy" ? amountCents - feeCents : amountCents + feeCents;
    if (baseCents <= 0) return null;
    return Math.round((baseCents * PRICE_UNITS_PER_FEN) / qty) / PRICE_UNITS_PER_YUAN;
  });

  const investmentAmount = computed(() => {
    if (quantityValue.value == null || priceValue.value == null) return 0;
    const feeValue = fee.value ?? 0;
    const raw =
      kind === "buy"
        ? quantityValue.value * priceValue.value + feeValue
        : quantityValue.value * priceValue.value - feeValue;
    return Math.round(raw * 100) / 100;
  });

  async function submit() {
    errors.markSaveAttempted();
    if (hasFieldError.value) return;
    if (!accountId.value) {
      message.warning(t("investments.form.selectAccount"));
      return;
    }
    if (!instrumentId.value) {
      message.warning(t("investments.form.selectInstrument"));
      return;
    }
    const fund = isFundInstrument.value;
    if (fund && (amount.value == null || amount.value <= 0)) {
      message.warning(
        t(kind === "buy" ? "investments.form.inputBuyAmount" : "investments.form.inputSellAmount"),
      );
      return;
    }
    const quantity = quantityValue.value;
    if (quantity == null) return;
    if (quantity <= 0) {
      message.warning(
        fund
          ? t("investments.form.inputShares")
          : t(
              kind === "buy"
                ? "investments.form.inputBuyQuantity"
                : "investments.form.inputSellQuantity",
            ),
      );
      return;
    }
    const price = priceValue.value;
    if (!fund) {
      if (price == null) return;
      if (price <= 0) {
        message.warning(
          t(kind === "buy" ? "investments.form.inputBuyPrice" : "investments.form.inputSellPrice"),
        );
        return;
      }
    }

    const editing = options?.editing?.() ?? null;
    try {
      const input = buildTradeInput({
        kind,
        currencyCode: currencyCode.value,
        accountId: accountId.value,
        instrumentId: instrumentId.value,
        amount: fund ? amount.value : null,
        quantity,
        price: fund ? null : price,
        fee: fee.value,
        fundingAccountId: fundingAccountId.value,
        note: note.value,
        sourceOrderNo: editing?.source_order_no ?? null,
        date: date.value,
      });
      if (editing) {
        await api.updateTransaction(editing.id, input);
        message.success(t("investments.form.saved"));
        options?.onUpdated?.();
      } else {
        await api.createTransaction(input);
        message.success(
          t(kind === "buy" ? "investments.form.recordedBuy" : "investments.form.recordedSell"),
        );
        instrumentId.value = null;
        amount.value = null;
        quantityText.value = "";
        priceText.value = "";
        errors.reset();
        fee.value = null;
        fundingAccountId.value = null;
        note.value = "";
        options?.onCreated?.();
      }
    } catch (e) {
      message.error(
        t(editing ? "investments.form.saveFailed" : "investments.form.recordFailed", {
          message: errorMessage(e),
        }),
      );
    }
  }

  function resetForm() {
    accountId.value = null;
    instrumentId.value = null;
    amount.value = null;
    quantityText.value = "";
    priceText.value = "";
    errors.reset();
    fee.value = null;
    fundingAccountId.value = null;
    note.value = "";
    date.value = Date.now();
  }

  return {
    accountId,
    fundingAccountId,
    fundingAccountOptions,
    instrumentId,
    amount,
    quantityText,
    priceText,
    fee,
    note,
    date,
    currencyCode,
    quantityError,
    priceError,
    hasFieldError,
    markQuantityBlurred,
    markPriceBlurred,
    isFundInstrument,
    derivedPrice,
    investmentAmount,
    investmentAccountOptions,
    instrumentOptions,
    currencyOptions,
    searchingInstruments,
    submit,
    searchInstruments,
    resetForm,
  };
}
