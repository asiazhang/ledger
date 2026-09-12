<script setup lang="ts">
import { computed } from 'vue'
import { NDescriptions, NDescriptionsItem } from 'naive-ui'
import { t } from '@ledger/i18n'
import { useReferenceStore } from '@/stores/reference'
import { formatQuantity } from '@/utils/money'
import type { Transaction, TransactionSplit } from '@ledger/types'

/**
 * 份额调整只读详情（ADR-0106 决策 10 / issue #1052）：split 是「无现金腿」kind，
 * 界面不体现任何写操作——无创建入口、无编辑、无软删；本组件只读呈现标的、
 * **带符号**份额变动 Δ、调整日与账户，写入与纠错仍走 HTTP 契约（AI / 迁移侧）。
 * 首行沿用 #1048 convert 详情形态：以 kind 标签开头（详情可辨识，ADR-0106 决策 13）。
 *
 * 份额变动按符号原样呈现（`+` = 折算 / 结转 / 送股，`-` = 缩股），取数口径单点
 * 在 `formatQuantity`（含数量分组与金额隐私模式的掩码恒等性），本组件不重算方向。
 */
const props = defineProps<{
  /** 份额调整交易行（提供日期 / 账户 / 备注） */
  transaction: Transaction
  /** 份额调整明细（`get_transaction_split` 读投影） */
  split: TransactionSplit
}>()

const reference = useReferenceStore()

/** 账户名经参考数据解析，未知账户回退占位（与列表账户列同口径，不抛错）。 */
const accountName = computed(
  () => reference.accountMap.get(props.transaction.account_id)?.name ?? '—',
)

/** 标的展示：代码 + 名称（名称缺失时仅代码）。 */
const instrumentText = computed(() =>
  props.split.instrument_name
    ? `${props.split.symbol} ${props.split.instrument_name}`
    : props.split.symbol,
)

/** 带符号份额变动：正向显式 `+`，负向沿用 `formatQuantity` 的 `-`；隐藏量级不隐藏方向。 */
const signedQuantityText = computed(() => {
  const magnitude = formatQuantity(Math.abs(props.split.quantity))
  return props.split.quantity < 0 ? `-${magnitude}` : `+${magnitude}`
})
</script>

<template>
  <NDescriptions :column="1" size="small" label-placement="left" bordered>
    <NDescriptionsItem :label="t('transactions.kind.split')">
      {{ instrumentText }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.splitShares')">
      {{ signedQuantityText }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.splitDate')">
      {{ transaction.date }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.account')">{{ accountName }}</NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.form.note')">
      {{ transaction.note ?? '—' }}
    </NDescriptionsItem>
  </NDescriptions>
</template>
