<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { NDataTable, NInput, NSelect, NSpace } from 'naive-ui'
import type { DataTableColumn } from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { formatAmount, INSTRUMENT_TYPE_LABELS, MARKET_TYPE_LABELS } from '@/types'
import type { Instrument, MarketType } from '@/types'

const store = useAppStore()

// 标的浏览（服务端分页 + 搜索）
const searchText = ref('')
const selectedMarket = ref<MarketType | null>(null)
const instruments = ref<Instrument[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = 50
const loading = ref(false)
let searchTimer: ReturnType<typeof setTimeout> | undefined

const marketOptions = computed(() =>
  (Object.entries(MARKET_TYPE_LABELS) as [MarketType, string][]).map(
    ([value, label]) => ({ label, value }),
  ),
)

async function load() {
  loading.value = true
  try {
    const res = await api.listInstruments({
      search: searchText.value.trim() || null,
      market: selectedMarket.value,
      page: page.value,
      page_size: pageSize,
    })
    instruments.value = res.items
    total.value = res.total
  } finally {
    loading.value = false
  }
}

function reload() {
  page.value = 1
  load()
}

watch(searchText, () => {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(reload, 300)
})
watch(selectedMarket, reload)

const pagination = computed(() => ({
  page: page.value,
  pageSize,
  itemCount: total.value,
  onChange: (p: number) => {
    page.value = p
    load()
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
      const ccy = store.currencyMap.get(row.currency_code)
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

onMounted(load)
</script>

<template>
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
      :data="instruments"
      :loading="loading"
      :bordered="false"
      size="small"
      remote
      :pagination="pagination"
    />
  </NSpace>
</template>
