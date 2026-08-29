<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { NCard, NSelect, NSpace, NEmpty, NSpin, NRadioGroup, NRadio, NText } from 'naive-ui'
import { Bar, Doughnut } from 'vue-chartjs'
import {
  Chart as ChartJS,
  Title,
  Tooltip,
  Legend,
  BarElement,
  ArcElement,
  CategoryScale,
  LinearScale,
} from 'chart.js'
import type { ChartOptions, TooltipItem } from 'chart.js'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount } from '@/types'
import type { CategoryShare, MerchantShare, MonthlySummary } from '@/types'
import { categoryRoot } from '@/utils/category-tree'
import {
  loadReportsGroupLevel,
  saveReportsGroupLevel,
  type ReportsGroupLevel,
} from '@/utils/view-state'
import MerchantRankingPanel from '@/components/reports/MerchantRankingPanel.vue'

ChartJS.register(Title, Tooltip, Legend, BarElement, ArcElement, CategoryScale, LinearScale)

const reference = useReferenceStore()
const year = ref(new Date().getFullYear())
const monthly = ref<MonthlySummary[]>([])
const shares = ref<CategoryShare[]>([])
const merchantShares = ref<MerchantShare[]>([])
const loading = ref(false)
const groupLevel = ref<ReportsGroupLevel>(loadReportsGroupLevel())

// ViewState：汇总层级跨启动保持。
watch(groupLevel, (v) => saveReportsGroupLevel(v))

async function refresh() {
  loading.value = true
  try {
    const [m, s, ms] = await Promise.all([
      api.monthlySummary(year.value),
      api.categoryShares('expense'),
      // 商户消费排行（issue #192）：随年份筛选，净额口径在后端收口
      api.merchantShares(year.value),
    ])
    monthly.value = m
    shares.value = s
    merchantShares.value = ms
  } finally {
    loading.value = false
  }
}

watch(year, refresh)

const barChartData = computed(() => ({
  labels: monthly.value.map((m) => m.month),
  datasets: [
    { label: '收入', data: monthly.value.map((m) => m.income_cents), backgroundColor: '#18a058' },
    { label: '支出', data: monthly.value.map((m) => m.expense_cents), backgroundColor: '#d03050' },
    { label: '退款', data: monthly.value.map((m) => m.refund_cents), backgroundColor: '#2080f0' },
  ],
}))

const barChartOptions: ChartOptions<'bar'> = {
  responsive: true,
  maintainAspectRatio: false,
  plugins: {
    legend: { position: 'top' },
    tooltip: {
      callbacks: {
        label: (context: TooltipItem<'bar'>) =>
          `${context.dataset.label}: ${formatAmount(context.raw as number)}`,
        afterBody: (items: TooltipItem<'bar'>[]) => {
          let income = 0
          let expense = 0
          let refund = 0
          for (const item of items) {
            if (item.dataset.label === '收入') income = item.raw as number
            if (item.dataset.label === '支出') expense = item.raw as number
            if (item.dataset.label === '退款') refund = item.raw as number
          }
          const net = income - expense + refund
          return `净额: ${formatAmount(net)}`
        },
      },
    },
  },
  scales: {
    y: {
      ticks: {
        callback: (value: number | string) => formatAmount(Number(value)),
      },
    },
  },
}

const pieData = computed(() => {
  if (groupLevel.value === 'level2') {
    return shares.value
      .filter((s) => s.amount_cents !== 0)
      .map((s) => ({ name: s.category_name, value: s.amount_cents }))
  }
  const map = new Map<string, { name: string; value: number }>()
  for (const s of shares.value) {
    if (s.amount_cents === 0) continue
    const root = categoryRoot(reference.categories, s.category_id)
    const key = root ? root.id : s.category_id
    const name = root ? root.name : s.category_name
    const exist = map.get(key)
    if (exist) exist.value += s.amount_cents
    else map.set(key, { name, value: s.amount_cents })
  }
  return Array.from(map.values())
})

const PALETTE = [
  '#5470c6', '#91cc75', '#fac858', '#ee6666', '#73c0de', '#3ba272',
  '#fc8452', '#9a60b4', '#ea7ccc', '#18a058', '#d03050', '#2080f0',
]

const doughnutChartData = computed(() => ({
  labels: pieData.value.map((d) => d.name),
  datasets: [
    {
      data: pieData.value.map((d) => d.value),
      backgroundColor: pieData.value.map((_, i) => PALETTE[i % PALETTE.length]),
      hoverOffset: 4,
    },
  ],
}))

const doughnutChartOptions: ChartOptions<'doughnut'> = {
  responsive: true,
  maintainAspectRatio: false,
  cutout: '40%',
  plugins: {
    legend: { position: 'right' },
    tooltip: {
      callbacks: {
        label: (context: TooltipItem<'doughnut'>) =>
          `${context.label}: ${formatAmount(context.raw as number)}`,
      },
    },
  },
}

onMounted(() => {
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll
  void refresh()
})
</script>

<template>
  <NSpin :show="loading">
    <NSpace vertical :size="16">
      <NSelect
        :value="year"
        :options="[year - 2, year - 1, year, year + 1].map((y) => ({ label: String(y), value: y }))"
        @update:value="(v: number) => (year = v)"
        style="width: 140px"
      />
      <NCard title="月度收支" size="small">
        <NEmpty v-if="monthly.length === 0" description="本年暂无数据" />
        <div v-else style="height: 320px">
          <Bar :data="barChartData" :options="barChartOptions" />
        </div>
      </NCard>
      <NCard title="支出分类占比" size="small">
        <NSpace v-if="shares.length > 0" align="center" :size="12" style="margin-bottom: 8px">
          <NText depth="3" style="font-size: 12px">汇总层级</NText>
          <NRadioGroup v-model:value="groupLevel" size="small">
            <NRadio value="level2">二级</NRadio>
            <NRadio value="level1">一级</NRadio>
          </NRadioGroup>
        </NSpace>
        <NEmpty v-if="shares.length === 0" description="暂无支出数据" />
        <div v-else style="height: 320px">
          <Doughnut :data="doughnutChartData" :options="doughnutChartOptions" />
        </div>
      </NCard>
      <MerchantRankingPanel :shares="merchantShares" />
    </NSpace>
  </NSpin>
</template>
