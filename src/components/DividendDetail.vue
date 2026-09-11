<script setup lang="ts">
import { computed } from 'vue'
import { NDescriptions, NDescriptionsItem } from 'naive-ui'
import { t } from '@/i18n'
import { useReferenceStore } from '@/stores/reference'
import { displayAmountText } from '@/components/transaction-columns'
import type { Transaction } from '@/types'

/**
 * 现金分红只读详情（ADR-0109 / issue #1078）：dividend 是界面只读 kind（比照
 * convert / split）——无创建入口、无编辑、无软删；本组件只读呈现归属标的、
 * 分红金额、到账账户与日期，写入与纠错仍走 HTTP 契约（AI / 迁移侧）。
 *
 * 金额展示复用列表口径单点 `displayAmountText`（含金额隐私模式与币种格式化），
 * 本组件不另算换算；归属标的取读投影的来源列（`source.kind === 'instrument'`，
 * 列表/搜索读路径填充），缺失回退占位不抛错。
 */
const props = defineProps<{
  /** 分红交易行（提供金额 / 币种 / 日期 / 账户 / 备注与来源列） */
  transaction: Transaction
}>()

const reference = useReferenceStore()

/** 账户名经参考数据解析，未知账户回退占位（与列表账户列同口径，不抛错）。 */
const accountName = computed(
  () => reference.accountMap.get(props.transaction.account_id)?.name ?? '—',
)

/** 归属标的：来源列标的反查的展示名（代码 + 名称），缺失回退占位。 */
const instrumentText = computed(() =>
  props.transaction.source?.kind === 'instrument' ? props.transaction.source.display_name : '—',
)

const amountText = computed(() => displayAmountText(reference, props.transaction))
</script>

<template>
  <NDescriptions :column="1" size="small" label-placement="left" bordered>
    <NDescriptionsItem :label="t('transactions.kind.dividend')">
      {{ instrumentText }}
    </NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.field.amount')">{{ amountText }}</NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.form.date')">{{ transaction.date }}</NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.field.account')">{{ accountName }}</NDescriptionsItem>
    <NDescriptionsItem :label="t('transactions.form.note')">
      {{ transaction.note ?? '—' }}
    </NDescriptionsItem>
  </NDescriptions>
</template>
