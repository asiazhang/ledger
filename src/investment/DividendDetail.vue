<script setup lang="ts">
import { computed } from "vue";
import { NDescriptions, NDescriptionsItem } from "naive-ui";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import { displayAmountText } from "@/transaction/transaction-columns";
import OrderSection from "@/transaction/OrderSection.vue";
import type { TransactionModalRow } from "@ledger/types";

const props = defineProps<{
  transaction: TransactionModalRow;
}>();

const reference = useReferenceStore();

const accountName = computed(
  () => reference.accountMap.get(props.transaction.account_id ?? "")?.name ?? "—",
);

const instrumentText = computed(() =>
  props.transaction.source?.kind === "instrument" ? props.transaction.source.display_name : "—",
);

const amountText = computed(() => displayAmountText(reference, props.transaction));
</script>

<template>
  <NDescriptions :column="1" size="small" label-placement="left" bordered>
    <NDescriptionsItem :label="t('transactions.kind.dividend')">
      {{ instrumentText }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.field.amount')">{{ amountText }}</NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.form.date')">{{
      transaction.date
    }}</NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.field.account')">{{
      accountName
    }}</NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.form.note')">
      {{ transaction.note ?? "—" }}
    </NDescriptionsItem>
  </NDescriptions>
  <!-- 所属订单区（#1862 / ADR-0138 决策 9）：来源订单号列有值才渲染——
         投资行（券商回单 buy/sell/convert/dividend）同样可有订单归属，
         点开任一同单行均可读订单汇总（两腿/分红详情与订单区并存） -->
  <OrderSection v-if="transaction.source_order_no" :source-order-no="transaction.source_order_no" />
</template>
