<script setup lang="ts">
import { computed, ref } from 'vue'
import { NAlert, NButton, NCard, NDataTable, NSpace, NText, useMessage } from 'naive-ui'
import { api } from '@ledger/api'
import { t } from '@ledger/i18n'
import { formatAmount } from '@ledger/money'
import { errorMessage } from '@/utils/errors'
import { useReferenceStore } from '@/stores/reference'
import type { BalanceCacheAudit } from '@ledger/types'

// 账户余额缓存卡片（ADR-0067 决策 5「手动审计兜底」的界面入口）：账户余额与
// 净资产读的是持久化缓存（V017），缓存缺失按裁决报码化错误、不静默回退实时
// 计算——修复动作因此不能藏在读路径里，只能由此处显式触发。修复语义全部在
// 命令层（全账户实时重算 vs 缓存逐户比对 → 整体重算回写 → 返回差异快照），
// 本组件只触发命令与呈现报告，不持第二份口径。

const message = useMessage()
const reference = useReferenceStore()

const report = ref<BalanceCacheAudit | null>(null)
const repairing = ref(false)

/** 漂移行的账户币种：账户名与币种都取自参考表（同一账户 id 的单一来源），
 *  参考表未收录该 id（如缓存行指向已删账户）时退化为无币种格式化。 */
function currencyOf(accountId: string) {
  const code = reference.accountMap.get(accountId)?.currency_code
  return code ? reference.getCurrency(code) : undefined
}

/** 缓存值列：缺失（null）是差异的一种形态，独立文案呈现，不显示成 0。 */
function cachedText(cents: number | null) {
  return cents === null ? t('settings.data.balanceCache.report.missing') : formatAmount(cents)
}

const columns = computed(() => [
  { title: () => t('settings.data.balanceCache.columns.account'), key: 'account_name' },
  {
    title: () => t('settings.data.balanceCache.columns.cached'),
    key: 'cached_cents',
    render: (row: BalanceCacheAudit['drifts'][number]) => cachedText(row.cached_cents),
  },
  {
    title: () => t('settings.data.balanceCache.columns.actual'),
    key: 'actual_cents',
    render: (row: BalanceCacheAudit['drifts'][number]) =>
      formatAmount(row.actual_cents, currencyOf(row.account_id)),
  },
])

/** 报告呈现形态：有漂移 → 已修复（列出逐户差异）；零漂移 → 无需修复。
 *  命令返回的报告恒为「修复已完成后的差异快照」，故不存在「未修复」态。 */
const reportView = computed(() => {
  const r = report.value
  if (!r) return null
  return r.drifts.length > 0
    ? {
        type: 'success' as const,
        title: t('settings.data.balanceCache.report.doneTitle', { n: r.drifts.length }),
        body: t('settings.data.balanceCache.report.doneBody'),
      }
    : {
        type: 'success' as const,
        title: t('settings.data.balanceCache.report.noopTitle'),
        body: t('settings.data.balanceCache.report.noopBody', { n: r.accounts_checked }),
      }
})

/** 触发一键修复：幂等，重复执行安全；报告就地覆盖呈现。 */
async function repair() {
  repairing.value = true
  try {
    report.value = await api.auditBalanceCache()
    message.success(reportView.value!.title)
  } catch (e: any) {
    message.error(t('settings.data.balanceCache.msg.repairFailed', { msg: errorMessage(e) }))
  } finally {
    repairing.value = false
  }
}
</script>

<template>
  <NCard :title="t('settings.data.balanceCache.title')" size="small">
    <NSpace vertical :size="12">
      <NText depth="3">
        {{ t('settings.data.balanceCache.hint') }}
      </NText>

      <NAlert
        v-if="reportView"
        :type="reportView.type"
        :show-icon="true"
        :title="reportView.title"
      >
        {{ reportView.body }}
      </NAlert>

      <NDataTable
        v-if="report && report.drifts.length > 0"
        :columns="columns"
        :data="report.drifts"
        :row-key="(row: BalanceCacheAudit['drifts'][number]) => row.account_id"
        :pagination="false"
        size="small"
        data-testid="balance-cache-drifts"
      />

      <NSpace align="center" :size="12">
        <NButton size="small" type="primary" :loading="repairing" @click="repair">
          {{ t('settings.data.balanceCache.repair') }}
        </NButton>
      </NSpace>
    </NSpace>
  </NCard>
</template>
