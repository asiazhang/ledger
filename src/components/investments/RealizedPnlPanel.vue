<script setup lang="ts">
import { h } from 'vue'
import {
  NCard,
  NDataTable,
  NEmpty,
  NGi,
  NGrid,
  NSpace,
  NSpin,
  NStatistic,
} from 'naive-ui'
import type { DataTableColumn } from 'naive-ui'
import PinyinSelect from '@/components/PinyinSelect.vue'
import InstrumentLink from '@/components/InstrumentLink.vue'
import { t } from '@ledger/i18n'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount } from '@ledger/money'
import { useRealizedPnl } from '@/composables/useRealizedPnl'
import { formatCurrencyGroups } from '@/composables/usePortfolioOverview'

const reference = useReferenceStore()
const {
  loading,
  summary,
  selectedAccountId,
  selectedInstrumentId,
  accountOptions,
  pnlInstrumentOptions,
  searchingInstruments,
  totalGroups,
  refresh,
  searchInstruments,
  onSelectInstrument,
} = useRealizedPnl()

// 汇总表通用「已实现盈亏」列：金额按行币种格式化展示（ADR-0107 决策 6：汇总行随
// 匹配行币种，与持仓页签行同款口径）；数值列右对齐 + 等宽数字（词汇表「表格列形态」
// 约定，三张汇总表同一单点收口）。
function realizedPnlColumn(title: string): DataTableColumn {
  return {
    title,
    key: 'realized_pnl_cents',
    align: 'right',
    className: 'tabular-nums',
    render(row: any) {
      return formatAmount(row.realized_pnl_cents, reference.currencyMap.get(row.currency_code))
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

const instPnlColumns: DataTableColumn[] = [
  {
    // 代码列下钻（ADR-0107 决策 5）：跳交易页 ?instrument=（不带账户）——本表含已
    // 清仓标的，是清仓标的卖出流水的唯一入口；标的字典无软删，跳转恒可达。
    title: t('investments.pnl.columns.symbol'),
    key: 'symbol',
    render(row: any) {
      return h(InstrumentLink, { instrumentId: row.instrument_id, label: row.symbol })
    },
  },
  { title: t('investments.pnl.columns.name'), key: 'name' },
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

      <NCard :title="t('investments.pnl.title')" size="small">
        <NEmpty v-if="!summary" :description="t('investments.pnl.empty')" />
        <!-- 总盈亏按币种分组展示（ADR-0107 决策 6/7）：逐组格式化「 / 」连接，不做跨币种
             折算；原单数字动画（NNumberAnimation）跨币种不适用，随混算口径一并退役，
             冗余的单统计网格（cols="1 s:2 m:4"）同步收敛为直接统计 -->
        <NStatistic v-else :label="t('investments.pnl.totalPnl')">
          {{ formatCurrencyGroups(totalGroups, reference.currencyMap) }}
        </NStatistic>
      </NCard>

      <template v-if="summary">
        <NGrid :x-gap="16" :y-gap="16" cols="1 s:2">
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

        <NCard :title="t('investments.pnl.byInstrument')" size="small">
          <NEmpty v-if="summary.by_instrument.length === 0" :description="t('investments.pnl.emptyTable')" />
          <NDataTable
            v-else
            :columns="instPnlColumns"
            :data="summary.by_instrument"
            :bordered="false"
            size="small"
          />
        </NCard>
      </template>
    </NSpace>
  </NSpin>
</template>
