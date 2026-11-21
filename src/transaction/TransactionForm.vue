<script setup lang="ts">
import { computed } from "vue";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import type { CreateFormKind, TransactionModalRow, TransactionTrade } from "@ledger/types";
import CategoryForm from "@/categories/CategoryForm.vue";
import TransferForm from "@/transaction/TransferForm.vue";
import LendingForm from "@/transaction/LendingForm.vue";
import InvestmentForm from "@/investment/InvestmentForm.vue";
import { isLendingEntryKind, resolveLendingDirection } from "@/transaction/lending";

const props = defineProps<{
  kind?: CreateFormKind;
  editing?: TransactionModalRow | null;
  trade?: TransactionTrade | null;
}>();

const emit = defineEmits<{ created: []; saved: [] }>();

const reference = useReferenceStore();

const effectiveKind = computed<CreateFormKind | null>(() =>
  props.editing ? (props.editing.kind as CreateFormKind) : (props.kind ?? null),
);

const editingLendingDirection = computed(() =>
  props.editing
    ? resolveLendingDirection(props.editing, (id) => reference.accountMap.get(id)?.type)
    : null,
);

function isLendingEntry(kind: CreateFormKind | null): kind is "lend" | "borrow" {
  return kind != null && isLendingEntryKind(kind);
}
</script>

<template>
  <div>
    <CategoryForm
      v-if="effectiveKind === 'expense'"
      kind="expense"
      :submit-label="t('transactions.form.submitExpense')"
      :editing="editing ?? null"
      @created="emit('created')"
      @saved="emit('saved')"
    />
    <CategoryForm
      v-if="effectiveKind === 'income'"
      kind="income"
      :submit-label="t('transactions.form.submitIncome')"
      :editing="editing ?? null"
      @created="emit('created')"
      @saved="emit('saved')"
    />
    <TransferForm
      v-if="effectiveKind === 'transfer' && !editingLendingDirection"
      :editing="editing ?? null"
      @created="emit('created')"
      @saved="emit('saved')"
    />
    <LendingForm
      v-else-if="isLendingEntry(effectiveKind)"
      :initial-direction="effectiveKind"
      @created="emit('created')"
    />
    <LendingForm
      v-else-if="effectiveKind === 'transfer' && editingLendingDirection"
      :editing="editing ?? null"
      :initial-direction="editingLendingDirection"
      @saved="emit('saved')"
    />
    <InvestmentForm
      v-if="effectiveKind === 'buy'"
      kind="buy"
      :submit-label="t('transactions.form.submitBuy')"
      :editing="editing ?? null"
      :trade="trade ?? null"
      @created="emit('created')"
      @saved="emit('saved')"
    />
    <InvestmentForm
      v-if="effectiveKind === 'sell'"
      kind="sell"
      :submit-label="t('transactions.form.submitSell')"
      :editing="editing ?? null"
      :trade="trade ?? null"
      @created="emit('created')"
      @saved="emit('saved')"
    />
  </div>
</template>
