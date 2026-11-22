<script setup lang="ts">
import { computed } from "vue";
import { NDescriptions, NDescriptionsItem } from "naive-ui";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import { formatAmount, formatQuantity } from "@ledger/money";
import { instrumentDisplayLabel } from "@/investment/instrument-display-label";
import OrderSection from "@/transaction/OrderSection.vue";
import type { TransactionModalRow, TransactionConvert } from "@ledger/types";

const props = defineProps<{
  transaction: TransactionModalRow;
  convert: TransactionConvert;
}>();

const reference = useReferenceStore();

const accountName = computed(
  () => reference.accountMap.get(props.transaction.account_id ?? "")?.name ?? "—",
);

function amountText(cents: number): string {
  return formatAmount(cents, reference.getCurrency(props.convert.currency_code));
}

function quantityText(quantity: number): string {
  return formatQuantity(quantity);
}
</script>

<template>
  <NDescriptions :column="1" size="small" label-placement="left" bordered>
    <NDescriptionsItem :label="t('transactions.kind.convert')">
      {{ convert.out_symbol }} → {{ convert.in_symbol }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.form.date')">{{
      transaction.date
    }}</NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.account')">{{ accountName }}</NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertOutInstrument')">
      {{ instrumentDisplayLabel(convert.out_symbol, convert.out_instrument_name) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertOutShares')">
      {{ quantityText(convert.out_quantity) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertOutAmount')">
      {{ amountText(convert.out_amount_cents) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertInInstrument')">
      {{ instrumentDisplayLabel(convert.in_symbol, convert.in_instrument_name) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertInShares')">
      {{ quantityText(convert.in_quantity) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertInAmount')">
      {{ amountText(convert.in_amount_cents) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.fee')">
      {{ amountText(convert.fee_cents) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertCarriedCost')">
      {{ amountText(convert.carried_cost_cents) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.form.note')">
      {{ transaction.note ?? "—" }}
    </NDescriptionsItem>
  </NDescriptions>
  <!-- 所属订单区（#1862 / ADR-0138 决策 9）：来源订单号列有值才渲染——
         投资行（券商回单 buy/sell/convert/dividend）同样可有订单归属，
         点开任一同单行均可读订单汇总（两腿/分红详情与订单区并存） -->
  <OrderSection v-if="transaction.source_order_no" :source-order-no="transaction.source_order_no" />
</template>
