<script setup lang="ts">
import { h, onMounted, ref, computed } from 'vue'
import {
  NCard,
  NSelect,
  NSpace,
  NSpin,
  NDataTable,
  NEmpty,
  NGi,
  NGrid,
  NStatistic,
  NNumberAnimation,
} from 'naive-ui'
import type { DataTableColumn } from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { formatAmount } from '@/types'
import type {
  RealizedPnlSummary,
  PnlDetail,
  Account,
  Instrument,
} from '@/types'

const store = useAppStore()
const loading = ref(false)
const summary = ref<RealizedPnlSummary | null>(null)
const accounts = ref<Account[]>([])
const instruments = ref<Instrument[]>([])
const selectedAccountId = ref<string | null>(null)
const selectedInstrumentId = ref<string | null>(null)

const accountOptions = computed(() =>
  accounts.value.map((a) => ({ label: a.name, value: a.id }))
)

const instrumentOptions = computed(() =>
  instruments.value.map((i) => ({
    label: `${i.symbol}${i.name ? ` - ${i.name}` : ''}`,
    value: i.id,
  }))
)

const currencyByCode = computed(() => store.currencyMap)

async function refresh() {
  loading.value = true
  try {
    const filter: Record<string, string | null> = {}
    if (selectedAccountId.value) filter.account_id = selectedAccountId.value
    if (selectedInstrumentId.value) filter.instrument_id = selectedInstrumentId.value
    summary.value = await api.realizedPnlSummary(
      Object.keys(filter).length > 0 ? filter : undefined
    )
  } finally {
    loading.value = false
  }
}

const totalPnl = computed(() => summary.value?.total_realized_pnl_cents ?? 0)

const detailColumns: DataTableColumn<PnlDetail>[] = [
  { title: '日期', key: 'sell_date', width: 120 },
  { title: '账户', key: 'account_name', width: 120 },
  { title: '标的', key: 'instrument_symbol', width: 100 },
  { title: '名称', key: 'instrument_name', width: 140 },
  { title: '数量', key: 'quantity', width: 80 },
  {
    title: '成本单价',
    key: 'cost_per_unit_cents',
    width: 100,
    render(row) {
      const ccy = currencyByCode.value.get(row.currency_code)
      return formatAmount(row.cost_per_unit_cents, ccy)
    },
  },
  {
    title: '已实现盈亏',
    key: 'realized_pnl_cents',
    width: 120,
    render(row) {
      const ccy = currencyByCode.value.get(row.currency_code)
      const text = formatAmount(row.realized_pnl_cents, ccy)
      return h(
        'span',
        { style: { color: row.realized_pnl_cents >= 0 ? '#18a058' : '#d03050' } },
        text
      )
    },
  },
]

const yearColumns: DataTableColumn[] = [
  { title: '年度', key: 'year' },
  {
    title: '已实现盈亏',
    key: 'realized_pnl_cents',
    render(row: any) {
      return formatAmount(row.realized_pnl_cents)
    },
  },
]

const accountColumns: DataTableColumn[] = [
  { title: '账户', key: 'account_name' },
  {
    title: '已实现盈亏',
    key: 'realized_pnl_cents',
    render(row: any) {
      return formatAmount(row.realized_pnl_cents)
    },
  },
]

const instrumentColumns: DataTableColumn[] = [
  { title: '代码', key: 'symbol' },
  { title: '名称', key: 'name' },
  {
    title: '已实现盈亏',
    key: 'realized_pnl_cents',
    render(row: any) {
      return formatAmount(row.realized_pnl_cents)
    },
  },
]

onMounted(async () => {
  await store.loadAll()
  accounts.value = await api.listAccounts()
  instruments.value = await api.listInstruments()
  await refresh()
})
</script>

<template>
  <NSpin :show="loading">
    <NSpace vertical :size="16">
      <NSpace align="center" :size="12">
        <NSelect
          v-model:value="selectedAccountId"
          :options="accountOptions"
          placeholder="全部账户"
          clearable
          style="width: 180px"
          @update:value="refresh"
        />
        <NSelect
          v-model:value="selectedInstrumentId"
          :options="instrumentOptions"
          placeholder="全部标的"
          clearable
          style="width: 220px"
          @update:value="refresh"
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
                :columns="accountColumns"
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
            :columns="instrumentColumns"
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
          />
        </NCard>
      </template>
    </NSpace>
  </NSpin>
</template>
