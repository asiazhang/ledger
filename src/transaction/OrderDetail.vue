<script setup lang="ts">
import DetailBasics from "@/transaction/DetailBasics.vue";
import OrderSection from "@/transaction/OrderSection.vue";
import type { TransactionModalRow } from "@ledger/types";

/**
 * 来源订单号行只读详情（issue #1862 / ADR-0138 决策 9）：不带出资分解与购买项、
 * 但来源订单号列有值的行（单账户订单的普通收支行、券商回单的 buy/sell 行等）的
 * 只读呈现面——基本信息（DetailBasics 共用块）+ 所属订单区（行数 · 合计 · 出资
 * 构成徽标 · 各行明细）。
 *
 * 与 FundingDetail / PurchaseDetail 的分工：分解行（fundings 非空）详情 =
 * FundingDetail；带购买项的行（purchases 非空）详情 = PurchaseDetail（判定先于
 * 订单号，清单面内嵌订单区）；本组件承接其余订单行，写入口（编辑 / 退款）不受
 * 影响——详情是增量呈现，菜单同时保留编辑通道。
 */
defineProps<{
  /** 交易行（弹窗族行投影，来源订单号随行携带） */
  row: TransactionModalRow;
}>();
</script>

<template>
  <div>
    <DetailBasics :row="row" />
    <OrderSection v-if="row.source_order_no" :source-order-no="row.source_order_no" />
  </div>
</template>
