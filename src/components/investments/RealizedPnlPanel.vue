<script setup lang="ts">
import { computed, h } from 'vue'
import { NCard, NDataTable, NEmpty, NGi, NGrid, NSpace, NSpin } from 'naive-ui'
import type { DataTableColumn } from 'naive-ui'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { t } from '@ledger/i18n'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { useWindowTier } from '@/composables/useWindowTier'
import { pnlSemanticColor } from '@/theme/semantic-colors'
import { formatAmount } from '@ledger/money'
import { useRealizedPnl } from '@/composables/useRealizedPnl'

const reference = useReferenceStore()
const appStore = useAppStore()
const windowTier = useWindowTier()
const isMobileTier = computed(() => windowTier.value === 'mobile')
const {
  loading,
  summary,
  selectedAccountId,
  selectedInstrumentId,
  accountOptions,
  pnlInstrumentOptions,
  searchingInstruments,
  refresh,
  searchInstruments,
  onSelectInstrument,
} = useRealizedPnl()

// 汇总表通用「已实现盈亏」列：金额按行币种格式化展示（ADR-0107 决策 6：汇总行随
// 匹配行币种，与持仓页签行同款口径）；数值列右对齐 + 等宽数字（词汇表「表格列形态」
// 约定，两张汇总表同一单点收口）；数字着盈亏涨跌色（红涨绿跌，与持仓页签
// 「持仓收益」列、合计三卡同一 semantic-colors 接缝），随主题取亮/暗变体。
function realizedPnlColumn(title: string): DataTableColumn {
  return {
    title,
    key: 'realized_pnl_cents',
    align: 'right',
    className: 'tabular-nums',
    render(row: any) {
      return h(
        'span',
        { style: { color: pnlSemanticColor(row.realized_pnl_cents, appStore.theme) } },
        formatAmount(row.realized_pnl_cents, reference.currencyMap.get(row.currency_code)),
      )
    },
  }
}

const yearColumns: DataTableColumn[] = [
  { title: t('investments.pnl.columns.year'), key: 'year' },
  realizedPnlColumn(t('investments.pnl.columns.realizedPnl')),
]

const accountCols: DataTableColumn[] = [
  { title: t('investments.pnl.columns.account'), key: 'account_name' },
  realizedPnlColumn(t('investments.pnl.columns.realizedPnl')),
]

</script>

<template>
  <NSpin :show="loading">
    <NSpace vertical :size="16">
      <NSpace align="center" :size="12">
        <PinyinSelect
          v-model:value="selectedAccountId"
          :options="accountOptions"
          :placeholder="t('investments.pnl.filterAccount')"
          clearable
          style="width: 180px"
          @update:value="refresh"
        />
        <!-- 远程搜索标的：拼音过滤由后端 list_instruments 统一语义（ADR-0027）
             承担，remote 下本地 filter 不生效，仅收口 filterable 保持载体一致。 -->
        <PinyinSelect
          v-model:value="selectedInstrumentId"
          :options="pnlInstrumentOptions"
          :placeholder="t('investments.pnl.filterInstrument')"
          remote
          clearable
          :loading="searchingInstruments"
          virtual-scroll
          style="width: 220px"
          @update:value="onSelectInstrument"
          @search="searchInstruments"
        />
      </NSpace>

      <!-- 页面收敛为 筛选 + 按年/按账户 两张汇总表（ADR-0107 修订注记，2026-09-13）：
           「已实现盈亏概览」卡与「按标的汇总」表退役——总口径在持仓页签合计（累计收益
           含已实现腿）可得，按标的信息在交易页标的筛选下钻可得。后端 realized_pnl_summary
           的 total / by_instrument 读取保留（只减 UI 面，不动 IPC 形状）。 -->
      <NEmpty v-if="!summary" :description="t('investments.pnl.empty')" />
      <template v-if="summary">
        <!-- 列数用纯数字 + 窗口分级：NGrid 默认 responsive="self" 只认数字前缀，
             具名断点（s:）永不命中会静默退成 1 列（两表竖排）。 -->
        <NGrid :x-gap="16" :y-gap="16" :cols="isMobileTier ? 1 : 2">
          <NGi>
            <NCard :title="t('investments.pnl.byYear')" size="small">
              <NEmpty v-if="summary.by_year.length === 0" :description="t('investments.pnl.emptyTable')" />
              <NDataTable
                v-else
                :columns="yearColumns"
                :data="summary.by_year"
                :bordered="false"
                size="small"
              />
            </NCard>
          </NGi>
          <NGi>
            <NCard :title="t('investments.pnl.byAccount')" size="small">
              <NEmpty v-if="summary.by_account.length === 0" :description="t('investments.pnl.emptyTable')" />
              <NDataTable
                v-else
                :columns="accountCols"
                :data="summary.by_account"
                :bordered="false"
                size="small"
              />
            </NCard>
          </NGi>
        </NGrid>
      </template>
    </NSpace>
  </NSpin>
</template>
