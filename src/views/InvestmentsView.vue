<script setup lang="ts">
import { h, onMounted, ref, computed, watch } from 'vue'
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
  NTabs,
  NTabPane,
  NInput,
} from 'naive-ui'
import type { DataTableColumn } from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { formatAmount, INSTRUMENT_TYPE_LABELS, MARKET_TYPE_LABELS } from '@/types'
import type {
  RealizedPnlSummary,
  PnlDetail,
  Account,
  Instrument,
  MarketType,
} from '@/types'

const store = useAppStore()
const loading = ref(false)
const summary = ref<RealizedPnlSummary | null>(null)
const accounts = ref<Account[]>([])
const selectedAccountId = ref<string | null>(null)
const selectedInstrumentId = ref<string | null>(null)

const accountOptions = computed(() =>
  accounts.value.map((a) => ({ label: a.name, value: a.id }))
)

// 盈亏 tab：标的筛选下拉（远程搜索，不前端全量驻留）
const searchInstrumentOptions = ref<{ label: string; value: string }[]>([])
const selectedInstrumentOption = ref<{ label: string; value: string } | null>(null)
const searchingInstruments = ref(false)
let instrumentSearchTimer: ReturnType<typeof setTimeout> | undefined

const pnlInstrumentOptions = computed(() => {
  const opts = [...searchInstrumentOptions.value]
  const sel = selectedInstrumentOption.value
  if (sel && !opts.some((o) => o.value === sel.value)) {
    opts.push(sel)
  }
  return opts
})

async function searchInstruments(query: string) {
  clearTimeout(instrumentSearchTimer)
  instrumentSearchTimer = setTimeout(async () => {
    if (!query.trim()) {
      searchInstrumentOptions.value = []
      return
    }
    searchingInstruments.value = true
    try {
      const res = await api.listInstruments({ search: query.trim(), page_size: 50 })
      searchInstrumentOptions.value = res.items.map((i) => ({
        label: `${i.symbol}${i.name ? ` - ${i.name}` : ''}`,
        value: i.id,
      }))
    } catch {
      searchInstrumentOptions.value = []
    } finally {
      searchingInstruments.value = false
    }
  }, 300)
}

function onSelectInstrument(value: string | null) {
  selectedInstrumentId.value = value
  selectedInstrumentOption.value =
    searchInstrumentOptions.value.find((o) => o.value === value) ?? null
  refresh()
}

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

const accountCols: DataTableColumn[] = [
  { title: '账户', key: 'account_name' },
  {
    title: '已实现盈亏',
    key: 'realized_pnl_cents',
    render(row: any) {
      return formatAmount(row.realized_pnl_cents)
    },
  },
]

const instPnlColumns: DataTableColumn[] = [
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

// 标的浏览 tab：服务端分页 + 搜索
const searchText = ref('')
const selectedMarket = ref<MarketType | null>(null)
const browseInstruments = ref<Instrument[]>([])
const browseTotal = ref(0)
const browsePage = ref(1)
const browsePageSize = 50
const browseLoading = ref(false)
let browseSearchTimer: ReturnType<typeof setTimeout> | undefined

const marketOptions = computed(() =>
  (Object.entries(MARKET_TYPE_LABELS) as [MarketType, string][]).map(
    ([value, label]) => ({ label, value }),
  )
)

async function loadBrowse() {
  browseLoading.value = true
  try {
    const res = await api.listInstruments({
      search: searchText.value.trim() || null,
      market: selectedMarket.value,
      page: browsePage.value,
      page_size: browsePageSize,
    })
    browseInstruments.value = res.items
    browseTotal.value = res.total
  } finally {
    browseLoading.value = false
  }
}

function reloadBrowse() {
  browsePage.value = 1
  loadBrowse()
}

watch(searchText, () => {
  clearTimeout(browseSearchTimer)
  browseSearchTimer = setTimeout(reloadBrowse, 300)
})
watch(selectedMarket, reloadBrowse)

const browsePagination = computed(() => ({
  page: browsePage.value,
  pageSize: browsePageSize,
  itemCount: browseTotal.value,
  onChange: (page: number) => {
    browsePage.value = page
    loadBrowse()
  },
}))

const instrumentBrowseColumns: DataTableColumn<Instrument>[] = [
  { title: '代码', key: 'symbol', width: 100 },
  { title: '名称', key: 'name', width: 200 },
  {
    title: '现价',
    key: 'price_cents',
    width: 100,
    render(row) {
      if (row.price_cents === null || row.price_cents === undefined) return '-'
      const ccy = currencyByCode.value.get(row.currency_code)
      return formatAmount(row.price_cents, ccy)
    },
  },
  {
    title: '市场',
    key: 'market',
    width: 80,
    render(row) {
      return MARKET_TYPE_LABELS[row.market] ?? row.market
    },
  },
  {
    title: '类型',
    key: 'type',
    width: 80,
    render(row) {
      return INSTRUMENT_TYPE_LABELS[row.type] ?? row.type
    },
  },
  { title: '币种', key: 'currency_code', width: 60 },
]

onMounted(async () => {
  await store.loadAll()
  accounts.value = await api.listAccounts()
  loadBrowse()
  await refresh()
})

// 切换 tab 时刷新数据
const activeTab = ref('pnl')
watch(activeTab, (tab) => {
  if (tab === 'instruments') {
    loadBrowse()
  }
})
</script>

<template>
  <NTabs v-model:value="activeTab" type="line">
    <NTabPane name="pnl" tab="盈亏">
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
              :options="pnlInstrumentOptions"
              placeholder="搜索标的"
              remote
              filterable
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
    </NTabPane>

    <NTabPane name="instruments" tab="标的">
      <NSpace vertical :size="12">
        <NSpace align="center" :size="12">
          <NInput
            v-model:value="searchText"
            placeholder="搜索代码或名称..."
            clearable
            style="width: 240px"
          />
          <NSelect
            v-model:value="selectedMarket"
            :options="marketOptions"
            placeholder="全部市场"
            clearable
            style="width: 140px"
          />
        </NSpace>
        <NDataTable
          :columns="instrumentBrowseColumns"
          :data="browseInstruments"
          :loading="browseLoading"
          :bordered="false"
          size="small"
          remote
          :pagination="browsePagination"
        />
      </NSpace>
    </NTabPane>
  </NTabs>
</template>
