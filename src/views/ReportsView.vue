<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { NCard, NSelect, NSpace, NEmpty, NSpin } from 'naive-ui'
import VChart from 'vue-echarts'
import { use } from 'echarts/core'
import { BarChart, PieChart } from 'echarts/charts'
import { GridComponent, TooltipComponent, LegendComponent, TitleComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { formatAmount } from '@/types'
import type { CategoryShare, MonthlySummary } from '@/types'

use([BarChart, PieChart, GridComponent, TooltipComponent, LegendComponent, TitleComponent, CanvasRenderer])

const store = useAppStore()
const year = ref(new Date().getFullYear())
const monthly = ref<MonthlySummary[]>([])
const shares = ref<CategoryShare[]>([])
const loading = ref(false)

async function refresh() {
  loading.value = true
  try {
    const [m, s] = await Promise.all([
      api.monthlySummary(year.value),
      api.categoryShares('expense'),
    ])
    monthly.value = m
    shares.value = s
  } finally {
    loading.value = false
  }
}

watch(year, refresh)

const barOption = () => ({
  tooltip: { trigger: 'axis' },
  legend: { data: ['收入', '支出'], top: 0 },
  grid: { left: 50, right: 20, top: 40, bottom: 30 },
  xAxis: { type: 'category', data: monthly.value.map((m) => m.month) },
  yAxis: {
    type: 'value',
    axisLabel: { formatter: (v: number) => formatAmount(v) },
  },
  series: [
    { name: '收入', type: 'bar', data: monthly.value.map((m) => m.income_cents) },
    { name: '支出', type: 'bar', data: monthly.value.map((m) => m.expense_cents) },
  ],
})

const pieOption = () => ({
  tooltip: { trigger: 'item', formatter: (p: { name: string; value: number }) => `${p.name}: ${formatAmount(p.value)}` },
  legend: { type: 'scroll', orient: 'vertical', right: 0, top: 'middle' },
  series: [
    {
      name: '支出分类',
      type: 'pie',
      radius: ['40%', '70%'],
      center: ['40%', '50%'],
      data: shares.value.map((s) => ({ name: s.category_name, value: s.amount_cents })),
    },
  ],
})

onMounted(async () => {
  await store.loadAll()
  await refresh()
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
        <VChart v-else :option="barOption()" style="height: 320px" autoresize />
      </NCard>
      <NCard title="支出分类占比" size="small">
        <NEmpty v-if="shares.length === 0" description="暂无支出数据" />
        <VChart v-else :option="pieOption()" style="height: 320px" autoresize />
      </NCard>
    </NSpace>
  </NSpin>
</template>
