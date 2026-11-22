<script setup lang="ts">
import { computed } from "vue";
import { NTag } from "naive-ui";
import { t } from "@ledger/i18n";
import { formatAmount } from "@ledger/money";
import { useReferenceStore } from "@/stores/reference";
import DetailBasics from "@/transaction/DetailBasics.vue";
import PurchaseSection from "@/transaction/PurchaseSection.vue";
import {
  FUNDING_SECTION_CLASS,
  FUNDING_SECTION_TITLE_CLASS,
  FUNDING_DERIVED_CLASS,
  FUNDING_ROW_CLASS,
  FUNDING_ACCOUNT_CLASS,
  FUNDING_LABEL_CLASS,
  FUNDING_AMOUNT_CLASS,
  FUNDING_TOTAL_CLASS,
} from "./funding-detail.css.ts";
import OrderSection from "@/transaction/OrderSection.vue";
import type { TransactionModalRow } from "@ledger/types";

const props = defineProps<{
  row: TransactionModalRow;
}>();

const reference = useReferenceStore();

function accountName(accountId: string): string {
  return reference.accountMap.get(accountId)?.name ?? "—";
}

function fundingAmount(cents: number): string {
  return formatAmount(cents, reference.getCurrency(props.row.currency_code));
}

const derived = computed(() => props.row.fundings.some((f) => f.derived));
</script>

<template>
  <div class="funding-detail">
    <DetailBasics :row="row" />
    <!-- 出资分解区：标注行 + 出资项列表 + Σ 合计行 -->
    <div :class="FUNDING_SECTION_CLASS">
      <div :class="FUNDING_SECTION_TITLE_CLASS">{{ t("transactions.funding.section") }}</div>
      <div v-if="derived" :class="FUNDING_DERIVED_CLASS">
        {{ t("transactions.funding.autoDerived") }}
      </div>
      <div v-for="(f, i) in row.fundings" :key="i" :class="FUNDING_ROW_CLASS">
        <span :class="FUNDING_ACCOUNT_CLASS">{{ accountName(f.account_id) }}</span>
        <NTag v-if="f.label" size="small" :bordered="true" :class="FUNDING_LABEL_CLASS">{{
          f.label
        }}</NTag>
        <span :class="FUNDING_AMOUNT_CLASS">{{ fundingAmount(f.amount_cents) }}</span>
      </div>
      <div :class="FUNDING_TOTAL_CLASS">
        <span>{{ t("transactions.funding.total", { n: row.fundings.length }) }}</span>
        <span :class="FUNDING_AMOUNT_CLASS">{{ fundingAmount(row.amount_cents) }}</span>
      </div>
    </div>
    <!-- 购买项清单区（issue #1884 / ADR-0138 决策 15）：分解行同时携带购买项时
         与出资项列表同层并陈（导入的多出资多商品订单形态）；无清单零变化 -->
    <PurchaseSection v-if="row.purchases.length > 0" :purchases="row.purchases" />
    <!-- 所属订单区（issue #1862 / ADR-0138 决策 9）：来源订单号列有值才渲染——
         同单聚合（行数 · 合计 · 出资构成徽标 · 各行明细）与本行出资项列表互补 -->
    <OrderSection v-if="row.source_order_no" :source-order-no="row.source_order_no" />
  </div>
</template>
