<script setup lang="ts">
import { computed } from "vue";
import { NDescriptions, NDescriptionsItem } from "naive-ui";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import { displayAmountText } from "@/transaction/transaction-columns";
import type { TransactionModalRow } from "@ledger/types";

const props = defineProps<{
  row: TransactionModalRow;
}>();

const reference = useReferenceStore();

const categoryText = computed(() =>
  props.row.category_id ? reference.categoryPath(props.row.category_id) || "—" : "—",
);
const merchantText = computed(() =>
  props.row.merchant_id ? (reference.merchantMap.get(props.row.merchant_id)?.name ?? "—") : "—",
);
const amountText = computed(() => displayAmountText(reference, props.row));
</script>

<template>
  <NDescriptions :column="1" size="small" label-placement="left" bordered>
    <NDescriptionsItem :label="t(`transactions.kind.${row.kind}`)">
      {{ categoryText }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.field.amount')">
      {{ amountText }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.field.merchant')">
      {{ merchantText }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.form.note')">
      {{ row.note ?? "—" }}
    </NDescriptionsItem>
  </NDescriptions>
</template>
