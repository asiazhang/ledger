<script setup lang="ts">
import { computed } from "vue";
import { NDescriptions, NDescriptionsItem } from "naive-ui";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import { displayAmountText } from "@/transaction/transaction-columns";
import type { TransactionModalRow } from "@ledger/types";

/**
 * 只读详情基本信息块（issue #1884 自出资 / 订单详情面收口）：kind 标签 + 分类、
 * 金额、商户、备注四项描述列表。详情面族（出资分解 / 订单 / 购买项）共用，
 * 金额走列表口径单点 `displayAmountText`，解析口径与收口前两面逐字一致
 * （组件级零行为变化，仅为购买项详情面免除第三份拷贝）。
 */
const props = defineProps<{
  /** 交易行（弹窗族行投影） */
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
  <NDescriptions :column="1" size="small" label-placement="left" bordered>
    <NDescriptionsItem :label="t(`transactions.kind.${row.kind}`)">
      {{ categoryText }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.field.amount')">
      {{ amountText }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.field.merchant')">
      {{ merchantText }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.form.note')">
      {{ row.note ?? "—" }}
    </NDescriptionsItem>
  </NDescriptions>
</template>
