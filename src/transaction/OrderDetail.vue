<script setup lang="ts">
import { computed } from "vue";
import { NDescriptions, NDescriptionsItem } from "naive-ui";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import { displayAmountText } from "@/transaction/transaction-columns";
import OrderSection from "@/transaction/OrderSection.vue";
import type { TransactionModalRow } from "@ledger/types";

/**
 * 来源订单号行只读详情（issue #1862 / ADR-0138 决策 9）：不带出资分解、但来源
 * 订单号列有值的行（单账户订单的普通收支行、券商回单的 buy/sell 行等）的只读
 * 呈现面——基本信息 + 所属订单区（行数 · 合计 · 出资构成徽标 · 各行明细）。
 *
 * 与 FundingDetail 的分工：分解行（fundings 非空）详情 = FundingDetail（基本信息
 * + 出资项列表 + 订单区）；本组件承接其余订单行（fundings 为空），写入口（编辑 /
 * 退款）不受影响——详情是增量呈现，菜单同时保留编辑通道。金额展示复用列表口径
 * 单点 `displayAmountText`。
 */
const props = defineProps<{
  /** 交易行（弹窗族行投影，来源订单号随行携带） */
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
  <div>
    <NDescriptions :column="1" size="small" label-placement="left" bordered>
      <NDescriptionsItem :label="t(`transactions.kind.${row.kind}`)">
        {{ categoryText }}
      </NDescriptionsItem>
      <NDescriptionsItem :label="t('transactions.field.amount')">{{
        amountText
      }}</NDescriptionsItem>
      <NDescriptionsItem :label="t('transactions.field.merchant')">
        {{ merchantText }}
      </NDescriptionsItem>
      <NDescriptionsItem :label="t('transactions.form.note')">
        {{ row.note ?? "—" }}
      </NDescriptionsItem>
    </NDescriptions>
    <OrderSection v-if="row.source_order_no" :source-order-no="row.source_order_no" />
  </div>
</template>
