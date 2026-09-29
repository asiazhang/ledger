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

/**
 * 出资分解只读详情（issue #1861 / ADR-0138 决策 8）：带非空出资分解的交易行
 * （分解 expense/income、显式覆盖与派生分解的 refund）的只读呈现面——基本信息
 * （DetailBasics 共用块）+ 出资项列表（账户 / 扣款标签徽标 / 金额）+ Σ 合计行；
 * 退款派生分解（读时推导、不落库，ADR-0138 决策 5）另带「按比例自动分解」标注；
 * 分解行同时携带购买项时，购买项清单区同层并陈（issue #1884 / ADR-0138 决策 15，
 * 与出资项列表对称）。
 *
 * 编辑 / 退款表单尚未支持分解（录入侧本票范围外），分解行不开放写入口，
 * 纠错走删除重建或 HTTP 契约。出资项金额为交易币种原始金额（Σ == 交易金额），
 * 经 formatAmount 按行币种格式化，隐私模式同层收口。
 */
const props = defineProps<{
  /** 交易行（弹窗族行投影，分解行随读回携带 fundings） */
  row: TransactionModalRow;
}>();

const reference = useReferenceStore();

/** 未知账户回退占位（与列表账户列同口径，不抛错）。 */
function accountName(accountId: string): string {
  return reference.accountMap.get(accountId)?.name ?? "—";
}

/** 行币种下的金额格式化（出资项为交易币种原始金额）。 */
function fundingAmount(cents: number): string {
  return formatAmount(cents, reference.getCurrency(props.row.currency_code));
}

/** 退款派生分解（读时推导、不落库）：任意条目 derived=true 即派生。 */
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
