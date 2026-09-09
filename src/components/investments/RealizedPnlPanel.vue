<script setup lang="ts">
import { h } from 'vue'
import {
  NCard,
  NDataTable,
  NEmpty,
  NGi,
  NGrid,
  NNumberAnimation,
  NSpace,
  NSpin,
  NStatistic,
} from 'naive-ui'
import type { DataTableColumn } from 'naive-ui'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { t } from '@/i18n'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount, formatPrice, formatQuantity } from '@/types'
import { pnlSemanticColor } from '@/theme/semantic-colors'
import type { PnlDetail } from '@/types'
import { useRealizedPnl } from '@/composables/useRealizedPnl'
import { sumFixedColumnWidths } from '@/utils/table'

const reference = useReferenceStore()
const {
  loading,
  summary,
  selectedAccountId,
  selectedInstrumentId,
  accountOptions,
  pnlInstrumentOptions,
  searchingInstruments,
  totalPnl,
  refresh,
  searchInstruments,
  onSelectInstrument,
} = useRealizedPnl()

// 卖出明细表列形态同持仓表（词汇表「表格列形态」约定）：名称列唯一弹性、
// 数值列右对齐 + 等宽数字，短内容列按内容定宽。
const detailColumns: DataTableColumn<PnlDetail>[] = [
  { title: t('investments.pnl.columns.date'), key: 'sell_date', width: 100, align: 'right', className: 'tabular-nums' },
  { title: t('investments.pnl.columns.account'), key: 'account_name', width: 110 },
  { title: t('investments.pnl.columns.instrument'), key: 'instrument_symbol', width: 100 },
  {
    // 唯一弹性列：不设固定宽，独吃窗口剩余宽度；minWidth 保窄窗口下限
    title: t('investments.pnl.columns.name'),
    key: 'instrument_name',
    minWidth: 140,
    ellipsis: { tooltip: true },
  },
  {
    title: t('investments.pnl.columns.quantity'),
    key: 'quantity',
    width: 110,
    align: 'right',
    className: 'tabular-nums',
    render: (row) => formatQuantity(row.quantity),
  },
  {
    title: t('investments.pnl.columns.costPrice'),
    key: 'cost_per_unit_cents',
    width: 110,
    align: 'right',
    className: 'tabular-nums',
    // 成本单价为价格列（万分之一元刻度，ADR-0038），用 formatPrice 展示
    render(row) {
      const ccy = reference.currencyMap.get(row.currency_code)
      return formatPrice(row.cost_per_unit_cents, ccy)
    },
  },
  {
    title: t('investments.pnl.columns.realizedPnl'),
    key: 'realized_pnl_cents',
    width: 130,
    align: 'right',
    className: 'tabular-nums',
    render(row) {
      const ccy = reference.currencyMap.get(row.currency_code)
      const text = formatAmount(row.realized_pnl_cents, ccy)
      // 红涨绿跌（A股/基金语境，词汇表「盈亏涨跌色」）；列配置为模块级常量，
      // 主题在渲染期经 store 响应式取值（transaction-columns 同款先例）
      return h(
        'span',
        { style: { color: pnlSemanticColor(row.realized_pnl_cents, useAppStore().theme) } },
        text,
      )
    },
  },
]

// 横向滚动下限 = 固定列宽总和（名称列 minWidth 不计入，与持仓表同规则）
const detailScrollX = sumFixedColumnWidths(detailColumns)

// 汇总表通用「已实现盈亏」列：金额按币种格式化展示；数值列右对齐 + 等宽数字
// （词汇表「表格列形态」约定，三张汇总表同一单点收口）。
function realizedPnlColumn(title: string): DataTableColumn {
  return {
    title,
    key: 'realized_pnl_cents',
    align: 'right',
    className: 'tabular-nums',
    render(row: any) {
      return formatAmount(row.realized_pnl_cents)
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
  { title: t('investments.pnl.columns.symbol'), key: 'symbol' },
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
        <NGrid v-else :x-gap="16" :y-gap="16" cols="1 s:2 m:4">
          <NGi>
            <NStatistic :label="t('investments.pnl.totalPnl')">
              <NNumberAnimation :from="0" :to="totalPnl" :duration="600" />
            </NStatistic>
          </NGi>
        </NGrid>
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

        <NCard :title="t('investments.pnl.details')" size="small">
          <NEmpty v-if="summary.details.length === 0" :description="t('investments.pnl.emptyTable')" />
          <NDataTable
            v-else
            :columns="detailColumns"
            :data="summary.details"
            :bordered="true"
            size="small"
            :scroll-x="detailScrollX"
            :pagination="{ pageSize: 20 }"
          />
        </NCard>
      </template>
    </NSpace>
  </NSpin>
</template>
