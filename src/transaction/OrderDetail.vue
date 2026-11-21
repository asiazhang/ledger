<script setup lang="ts">
import DetailBasics from "@/transaction/DetailBasics.vue";
import OrderSection from "@/transaction/OrderSection.vue";
import type { TransactionModalRow } from "@ledger/types";

defineProps<{
  row: TransactionModalRow;
}>();
</script>

<template>
  <div>
    <DetailBasics :row="row" />
    <OrderSection v-if="row.source_order_no" :source-order-no="row.source_order_no" />
  </div>
</template>
