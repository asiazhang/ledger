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

/**
 * 购买项清单区（issue #1884 / ADR-0138 决策 15）：只读详情弹窗的购买项呈现面——
 * 区标题 + 逐条「名称 / 分类 / 件数」。与出资项列表同层对称（出资详情面与购买项
 * 详情面共用本区）；商品行渲染与件数文案消费列表侧单点 renderPurchaseLine 全文
 * 形态（触控轴没有悬停，详情是长商品名的全文出口）。
 * 分类经参考数据单源解析（未知 / 已删分类与无分类统一占位「—」）；价格为
 * 存而不显示字段，不在本区出现（ADR-0138 决策 10）。
 */
defineProps<{
  /** 购买项清单（弹窗族行投影随读回携带，对账单顺序） */
  purchases: TransactionPurchase[];
}>();

const reference = useReferenceStore();

/** 分类路径（与列表分类列同一单源解析，占位符随详情面「—」惯例）。 */
function categoryText(categoryId: string | null): string {
  return categoryId ? reference.categoryPath(categoryId) || "—" : "—";
}

/** 行渲染适配：列表侧渲染单点的全文形态 + 分类弱化标注，经功能组件进模板。 */
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
