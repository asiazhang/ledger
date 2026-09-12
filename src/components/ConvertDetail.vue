<script setup lang="ts">
import { computed } from 'vue'
import { NDescriptions, NDescriptionsItem } from 'naive-ui'
import { t } from '@/i18n'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount, formatQuantity } from '@/utils/money'
import type { Transaction, TransactionConvert } from '@ledger/types'

/**
 * 基金转换只读详情（ADR-0106 决策 10 / #1048）：convert 是「无现金腿」kind，界面不体现
 * 任何写操作——无创建入口、无编辑、无软删；本组件只读呈现「A → B」两侧标的、份额、
 * 金额、手续费与结转成本，写入与纠错仍走 HTTP 契约（AI / 迁移侧）。
 *
 * 金额展示统一走 formatAmount / formatQuantity 单点（含金额隐私模式）；行金额锚点是
 * 结转成本而非确认单金额，列表展示口径（转出金额）与锚点在此分列呈现，避免混淆。
 */
const props = defineProps<{
  /** 转换交易行（提供日期 / 账户 / 备注） */
  transaction: Transaction
  /** 转换两腿明细（`get_transaction_convert` 读投影） */
  convert: TransactionConvert
}>()

const reference = useReferenceStore()

/** 账户名经参考数据解析，未知账户回退占位（与列表账户列同口径，不抛错）。 */
const accountName = computed(
  () => reference.accountMap.get(props.transaction.account_id)?.name ?? '—',
)

function amountText(cents: number): string {
  return formatAmount(cents, reference.getCurrency(props.convert.currency_code))
}

function quantityText(quantity: number): string {
  return formatQuantity(quantity)
}

/** 标的展示：代码 + 名称（名称缺失时仅代码）。 */
function instrumentText(symbol: string, name: string | null): string {
  return name ? `${symbol} ${name}` : symbol
}
</script>

<template>
  <NDescriptions :column="1" size="small" label-placement="left" bordered>
    <NDescriptionsItem :label="t('transactions.kind.convert')">
      {{ convert.out_symbol }} → {{ convert.in_symbol }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.form.date')">{{ transaction.date }}</NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.account')">{{ accountName }}</NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertOutInstrument')">
      {{ instrumentText(convert.out_symbol, convert.out_instrument_name) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertOutShares')">
      {{ quantityText(convert.out_quantity) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertOutAmount')">
      {{ amountText(convert.out_amount_cents) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertInInstrument')">
      {{ instrumentText(convert.in_symbol, convert.in_instrument_name) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertInShares')">
      {{ quantityText(convert.in_quantity) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertInAmount')">
      {{ amountText(convert.in_amount_cents) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.fee')">
      {{ amountText(convert.fee_cents) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('investments.form.convertCarriedCost')">
      {{ amountText(convert.carried_cost_cents) }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.form.note')">
      {{ transaction.note ?? '—' }}
    </NDescriptionsItem>
  </NDescriptions>
</template>
