<script setup lang="ts">
import type { FunctionalComponent } from "vue";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import { renderPurchaseLine } from "@/transaction/transaction-columns";
import type { TransactionPurchase } from "@ledger/types";
import {
  PURCHASE_SECTION_CLASS,
  PURCHASE_SECTION_TITLE_CLASS,
  PURCHASE_ROW_CLASS,
} from "./purchase-section.css.ts";

defineProps<{
  purchases: TransactionPurchase[];
}>();

const reference = useReferenceStore();

function categoryText(categoryId: string | null): string {
  return categoryId ? reference.categoryPath(categoryId) || "—" : "—";
}

const PurchaseLine: FunctionalComponent<{ item: TransactionPurchase }> = (cellProps) =>
  renderPurchaseLine(cellProps.item, { suffix: categoryText(cellProps.item.category_id) });
PurchaseLine.props = { item: { type: Object, required: true } };
</script>

<template>
  <div :class="PURCHASE_SECTION_CLASS">
    <div :class="PURCHASE_SECTION_TITLE_CLASS">{{ t("transactions.purchase.section") }}</div>
    <div v-for="(item, i) in purchases" :key="i" :class="PURCHASE_ROW_CLASS">
      <PurchaseLine :item="item" />
    </div>
  </div>
</template>
