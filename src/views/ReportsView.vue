<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { NCard, NSelect, NSpace, NEmpty, NSpin, NRadioGroup, NRadio, NText } from 'naive-ui'
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
const groupLevel = ref<'level1' | 'level2'>('level2')

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
  tooltip: {
    trigger: 'axis',
    formatter: (params: { seriesName: string; value: number; axisValue: string }[]) => {
      let income = 0
      let expense = 0
      let refund = 0
      for (const p of params) {
        if (p.seriesName === '收入') income = p.value
        if (p.seriesName === '支出') expense = p.value
        if (p.seriesName === '退款') refund = p.value
      }
      const net = income - expense + refund
      return `${params[0]?.axisValue ?? ''}<br/>收入: ${formatAmount(income)}<br/>支出: ${formatAmount(expense)}<br/>退款: ${formatAmount(refund)}<br/>净额: ${formatAmount(net)}`
    },
  },
  legend: { data: ['收入', '支出', '退款'], top: 0 },
  grid: { left: 50, right: 20, top: 40, bottom: 30 },
  xAxis: { type: 'category', data: monthly.value.map((m) => m.month) },
  yAxis: {
    type: 'value',
    axisLabel: { formatter: (v: number) => formatAmount(v) },
  },
  series: [
    { name: '收入', type: 'bar', data: monthly.value.map((m) => m.income_cents) },
    { name: '支出', type: 'bar', data: monthly.value.map((m) => m.expense_cents) },
    { name: '退款', type: 'bar', data: monthly.value.map((m) => m.refund_cents) },
  ],
})

// 支出分类饼图数据：level2 用二级分类，level1 上卷到顶级分类
const pieData = computed(() => {
  if (groupLevel.value === 'level2') {
    return shares.value
      .filter((s) => s.amount_cents !== 0)
      .map((s) => ({ name: s.category_name, value: s.amount_cents }))
  }
  const map = new Map<string, { name: string; value: number }>()
  for (const s of shares.value) {
    if (s.amount_cents === 0) continue
    const cat = store.categoryMap.get(s.category_id)
    const root = cat && cat.parent_id != null
      ? (store.categoryMap.get(cat.parent_id) ?? cat)
      : cat
    const key = root ? root.id : s.category_id
    const name = root ? root.name : s.category_name
    const exist = map.get(key)
    if (exist) exist.value += s.amount_cents
    else map.set(key, { name, value: s.amount_cents })
  }
  return Array.from(map.values())
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
      data: pieData.value,
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
        <NSpace v-if="shares.length > 0" align="center" :size="12" style="margin-bottom: 8px">
          <NText depth="3" style="font-size: 12px">汇总层级</NText>
          <NRadioGroup v-model:value="groupLevel" size="small">
            <NRadio value="level2">二级</NRadio>
            <NRadio value="level1">一级</NRadio>
          </NRadioGroup>
        </NSpace>
        <NEmpty v-if="shares.length === 0" description="暂无支出数据" />
        <VChart v-else :option="pieOption()" style="height: 320px" autoresize />
      </NCard>
    </NSpace>
  </NSpin>
</template>
