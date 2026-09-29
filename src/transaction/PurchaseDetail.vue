<script setup lang="ts">
import DetailBasics from "@/transaction/DetailBasics.vue";
import PurchaseSection from "@/transaction/PurchaseSection.vue";
import OrderSection from "@/transaction/OrderSection.vue";
import type { TransactionModalRow } from "@ledger/types";

/**
 * 购买项行只读详情（issue #1884 / ADR-0138 决策 15）：带非空购买项清单的 expense 行
 * （fundings 为空）的只读呈现面——基本信息 + 购买项清单（名称 / 分类 / 件数）+
 * 所属订单区（来源订单号有值时与清单互补；购买项详情判定先于订单号，本面是 order
 * 面的超集）。与 FundingDetail 的分工：分解行（fundings 非空）详情 = FundingDetail，
 * 购买项清单由其同层并陈；本组件承接其余购买项行。
 * 界面只读（不提供购买项编辑入口，纠错靠按幂等键重导），金额展示复用列表口径单点。
 */
defineProps<{
  /** 交易行（弹窗族行投影，购买项清单随读回携带） */
  row: TransactionModalRow;
}>();
</script>

<template>
  <div>
    <DetailBasics :row="row" />
    <PurchaseSection :purchases="row.purchases" />
    <OrderSection v-if="row.source_order_no" :source-order-no="row.source_order_no" />
  </div>
</template>
