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
import { useReferenceStore } from '@/stores/reference'
import { formatAmount, formatPrice, formatQuantity } from '@/types'
import type { PnlDetail } from '@/types'
import { useRealizedPnl } from '@/composables/useRealizedPnl'
import HoldingsOverview from '@/components/investments/HoldingsOverview.vue'

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

const detailColumns: DataTableColumn<PnlDetail>[] = [
  { title: '日期', key: 'sell_date', width: 120 },
  { title: '账户', key: 'account_name', width: 120 },
  { title: '标的', key: 'instrument_symbol', width: 100 },
  { title: '名称', key: 'instrument_name', width: 140 },
  { title: '数量', key: 'quantity', width: 80, render: (row) => formatQuantity(row.quantity) },
  {
    title: '成本单价',
    key: 'cost_per_unit_cents',
    width: 100,
    // 成本单价为价格列（万分之一元刻度，ADR-0038），用 formatPrice 展示
    render(row) {
      const ccy = reference.currencyMap.get(row.currency_code)
      return formatPrice(row.cost_per_unit_cents, ccy)
    },
  },
  {
    title: '已实现盈亏',
    key: 'realized_pnl_cents',
    width: 120,
    render(row) {
      const ccy = reference.currencyMap.get(row.currency_code)
      const text = formatAmount(row.realized_pnl_cents, ccy)
      return h(
        'span',
        { style: { color: row.realized_pnl_cents >= 0 ? '#18a058' : '#d03050' } },
        text,
      )
    },
  },
]

// 汇总表通用「已实现盈亏」列：金额按币种格式化展示。
function realizedPnlColumn(title: string): DataTableColumn {
  return {
    title,
    key: 'realized_pnl_cents',
    render(row: any) {
      return formatAmount(row.realized_pnl_cents)
    },
  }
}

const yearColumns: DataTableColumn[] = [
  { title: '年度', key: 'year' },
  realizedPnlColumn('已实现盈亏'),
]

const accountCols: DataTableColumn[] = [
  { title: '账户', key: 'account_name' },
  realizedPnlColumn('已实现盈亏'),
]

const instPnlColumns: DataTableColumn[] = [
  { title: '代码', key: 'symbol' },
  { title: '名称', key: 'name' },
  realizedPnlColumn('已实现盈亏'),
]
</script>

<template>
  <NSpin :show="loading">
    <NSpace vertical :size="16">
      <!-- 当前持仓概览（issue #110）：总市值/未实现盈亏合计 + 持仓明细 + 同步持仓价格按钮 -->
      <HoldingsOverview />

      <NSpace align="center" :size="12">
        <PinyinSelect
          v-model:value="selectedAccountId"
          :options="accountOptions"
          placeholder="全部账户"
          clearable
          style="width: 180px"
          @update:value="refresh"
        />
        <!-- 远程搜索标的：拼音过滤由后端 list_instruments 统一语义（ADR-0027）
             承担，remote 下本地 filter 不生效，仅收口 filterable 保持载体一致。 -->
        <PinyinSelect
          v-model:value="selectedInstrumentId"
          :options="pnlInstrumentOptions"
          placeholder="搜索标的（代码/名称/拼音）"
          remote
          clearable
          :loading="searchingInstruments"
          virtual-scroll
          style="width: 220px"
          @update:value="onSelectInstrument"
          @search="searchInstruments"
        />
      </NSpace>

      <NCard title="已实现盈亏概览" size="small">
        <NEmpty v-if="!summary" description="暂无已实现盈亏数据" />
        <NGrid v-else :x-gap="16" :y-gap="16" cols="1 s:2 m:4">
          <NGi>
            <NStatistic label="总已实现盈亏">
              <NNumberAnimation :from="0" :to="totalPnl" :duration="600" />
            </NStatistic>
          </NGi>
        </NGrid>
      </NCard>

      <template v-if="summary">
        <NGrid :x-gap="16" :y-gap="16" cols="1 s:2">
          <NGi>
            <NCard title="按年度汇总" size="small">
              <NEmpty v-if="summary.by_year.length === 0" description="暂无数据" />
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
            <NCard title="按账户汇总" size="small">
              <NEmpty v-if="summary.by_account.length === 0" description="暂无数据" />
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

        <NCard title="按标的汇总" size="small">
          <NEmpty v-if="summary.by_instrument.length === 0" description="暂无数据" />
          <NDataTable
            v-else
            :columns="instPnlColumns"
            :data="summary.by_instrument"
            :bordered="false"
            size="small"
          />
        </NCard>

        <NCard title="卖出明细" size="small">
          <NEmpty v-if="summary.details.length === 0" description="暂无数据" />
          <NDataTable
            v-else
            :columns="detailColumns"
            :data="summary.details"
            :bordered="true"
            size="small"
            :pagination="{ pageSize: 20 }"
          />
        </NCard>
      </template>
    </NSpace>
  </NSpin>
</template>
