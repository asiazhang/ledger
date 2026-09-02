<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { NCard, NSpace, NEmpty, NSpin, NBreadcrumb, NBreadcrumbItem } from 'naive-ui'
import AppSelect from '@/components/AppSelect.vue'
import { Bar } from 'vue-chartjs'
import {
  Chart as ChartJS,
  Title,
  Tooltip,
  Legend,
  BarElement,
  CategoryScale,
  LinearScale,
} from 'chart.js'
import type { ActiveElement, Chart, ChartOptions, TooltipItem } from 'chart.js'
import { api } from '@/api'
import { t } from '@/i18n'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount } from '@/types'
import type { CategoryShare, MerchantShare, MonthlySummary, ReportDateRange } from '@/types'
import {
  barEndLabel,
  categoryBarTotal,
  categoryBars,
  categoryDrilldownBars,
} from '@/utils/category-chart'
import { categoryRoot } from '@/utils/category-tree'
import { derivePeriodBoundary } from '@/utils/time-period'
import MerchantRankingPanel from '@/components/reports/MerchantRankingPanel.vue'

ChartJS.register(Title, Tooltip, Legend, BarElement, CategoryScale, LinearScale)

const reference = useReferenceStore()
const year = ref(new Date().getFullYear())
// 报表年份筛选（issue #267 / #389）：可选范围由前端期间数学单点派生年档闭区间
// [最早交易年份, max(当前年, 最新交易年份)]，空库回退单当前年；
// 范围内年份升序平铺直选，任何年份一击直达（取代围绕已选年份 ±2 的滑动窗口）。
const dateRange = ref<ReportDateRange | null>(null)
const yearOptions = computed(() => {
  if (!dateRange.value) return []
  const bounds = derivePeriodBoundary('year', dateRange.value, new Date())
  const options: { label: string; value: number }[] = []
  for (let y = bounds.earliest.year; y <= bounds.latest.year; y++) {
    options.push({ label: String(y), value: y })
  }
  return options
})
const monthly = ref<MonthlySummary[]>([])
const shares = ref<CategoryShare[]>([])
const merchantShares = ref<MerchantShare[]>([])
const loading = ref(false)
// 图内下钻态（issue #379）：当前下钻的一级分类 id，null = 基础态；
// 视图内瞬时状态，不持久化（与 ViewState 边界一致），年份切换复位。
const drilledRootId = ref<string | null>(null)

async function refresh() {
  loading.value = true
  try {
    const [m, s, ms] = await Promise.all([
      api.monthlySummary(year.value),
      // 分类份额随年份筛选联动（issue #376）：三张报表口径一致，
      // 净值口径在后端收口
      api.categoryShares('expense', { year: year.value }),
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

watch(year, () => {
  // 年份切换复位下钻（issue #379）：下钻是视图内瞬时状态，
  // 停留在新年份无数据的悬停下钻态只会迷惑，回基础态重看全貌
  drilledRootId.value = null
  refresh()
})

const barChartData = computed(() => ({
  labels: monthly.value.map((m) => m.month),
  // dataset 顺序固定：0=收入、1=支出、2=退款；tooltip 回调按 datasetIndex 取值，
  // 不依赖 label 字符串（label 已随界面语言迁移，不能再当判断依据）。
  datasets: [
    { label: t('reports.monthly.income'), data: monthly.value.map((m) => m.income_cents), backgroundColor: '#18a058' },
    { label: t('reports.monthly.expense'), data: monthly.value.map((m) => m.expense_cents), backgroundColor: '#d03050' },
    { label: t('reports.monthly.refund'), data: monthly.value.map((m) => m.refund_cents), backgroundColor: '#2080f0' },
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
            if (item.datasetIndex === 0) income = item.raw as number
            if (item.datasetIndex === 1) expense = item.raw as number
            if (item.datasetIndex === 2) refund = item.raw as number
          }
          const net = income - expense + refund
          return t('reports.monthly.net', { amount: formatAmount(net) })
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

// 支出分类构成（issue #378）：横向柱状图。一级归并 + 未分类柱、净额降序、
// 按 id 稳定配色、未分类灰——数据形态收口在 category-chart 纯函数，此处只消费。
// 图内下钻（issue #379）：点一级柱（未分类除外）切到该分类的二级构成
// （二级子分类 + 直挂行）。
const drilledRoot = computed(() =>
  drilledRootId.value
    ? (reference.categories.find((c) => c.id === drilledRootId.value) ?? null)
    : null,
)

const categoryBarsData = computed(() => {
  const root = drilledRoot.value
  if (root) {
    return categoryDrilldownBars(
      shares.value,
      reference.categories,
      root.id,
      t('reports.category.direct', { name: root.name }),
    )
  }
  return categoryBars(shares.value, reference.categories)
})

/** 点柱下钻分派（issue #379）：仅基础态下、参考数据可解析的一级分类触发；
 *  未分类柱本票不响应（跳转接线归后续票），下钻态内点击不变态。
 *  守卫读已解析的 drilledRoot：下钻分类中途被删时自然回基础态、点击不卡死。 */
function handleCategoryBarClick(_event: unknown, elements: ActiveElement[]) {
  if (drilledRoot.value) return
  const bar = categoryBarsData.value[elements[0]?.index ?? -1]
  if (!bar?.id) return
  if (!categoryRoot(reference.categories, bar.id)) return
  drilledRootId.value = bar.id
}

const categoryChartData = computed(() => ({
  labels: categoryBarsData.value.map((b) => b.name),
  datasets: [
    {
      data: categoryBarsData.value.map((b) => b.value),
      backgroundColor: categoryBarsData.value.map((b) => b.color),
      barThickness: 20,
    },
  ],
}))

// 平铺滚动：图高随行数增长（全部分类不截断），卡片内限高滚动。
const CATEGORY_ROW_HEIGHT = 32
const CATEGORY_MIN_ROWS = 6
const categoryChartHeight = computed(() => {
  const rows = Math.max(CATEGORY_MIN_ROWS, categoryBarsData.value.length)
  return rows * CATEGORY_ROW_HEIGHT
})

// 柱尾「金额 · 占比%」标签：占比分母 = 全部一级柱合计（图行净额代数和，负柱如实冲减）。
const barEndLabelPlugin = {
  id: 'barEndLabels',
  afterDatasetsDraw(chart: Chart<'bar'>) {
    const data = chart.data.datasets[0]?.data as number[] | undefined
    if (!data?.length) return
    const total = categoryBarTotal(categoryBarsData.value)
    const ctx = chart.ctx
    ctx.save()
    ctx.fillStyle = typeof chart.options.color === 'string' ? chart.options.color : '#666'
    const f = ChartJS.defaults.font
    ctx.font = `${f.size ?? 12}px ${f.family ?? 'sans-serif'}`
    ctx.textBaseline = 'middle'
    chart.getDatasetMeta(0).data.forEach((el, i) => {
      const value = data[i]
      const { x, y } = el.getProps(['x', 'y'], true)
      // 正值柱标在柱尾右侧，负值柱标在柱尾左侧（0 轴如实渲染）
      ctx.textAlign = value >= 0 ? 'left' : 'right'
      ctx.fillText(barEndLabel(value, total), value >= 0 ? x + 6 : x - 6, y)
    })
    ctx.restore()
  },
}

const categoryChartOptions: ChartOptions<'bar'> = {
  indexAxis: 'y',
  responsive: true,
  maintainAspectRatio: false,
  // 两端留白容柱尾标签（x 轴 grace 把最大/最小值两端各拓 30%）
  layout: { padding: { left: 4, right: 8 } },
  onClick: handleCategoryBarClick,
  plugins: {
    legend: { display: false },
    tooltip: {
      callbacks: {
        label: (context: TooltipItem<'bar'>) => formatAmount(context.raw as number),
      },
    },
  },
  scales: {
    x: {
      type: 'linear',
      grace: '30%',
      ticks: {
        callback: (value: number | string) => formatAmount(Number(value)),
      },
    },
    y: {
      // 平铺不截断：全部类目标签都画，行多时容器滚动
      ticks: { autoSkip: false },
    },
  },
}

// 范围挂载时拉取一次（不接失效信号：进视图即新鲜，跨会话自然生效）
async function loadDateRange() {
  dateRange.value = await api.reportDateRange()
}

onMounted(() => {
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll
  void loadDateRange()
  void refresh()
})
</script>

<template>
  <NSpin :show="loading">
    <NSpace vertical :size="16">
      <AppSelect
        :value="year"
        :options="yearOptions"
        @update:value="(v: number) => (year = v)"
        style="width: 140px"
      />
      <NCard :title="t('reports.monthly.title')" size="small">
        <NEmpty v-if="monthly.length === 0" :description="t('reports.monthly.empty')" />
        <div v-else style="height: 320px">
          <Bar :data="barChartData" :options="barChartOptions" />
        </div>
      </NCard>
      <NCard size="small">
        <template #header>
          <div class="category-card-header">
            <span>{{ t('reports.category.title') }}</span>
            <!-- 面包屑（issue #379）：下钻态显示当前位置，点根返回基础态 -->
            <NBreadcrumb v-if="drilledRoot" data-testid="category-breadcrumb" separator="›">
              <NBreadcrumbItem @click="drilledRootId = null">
                <span data-testid="breadcrumb-root">{{ t('reports.category.all') }}</span>
              </NBreadcrumbItem>
              <NBreadcrumbItem>
                <span data-testid="breadcrumb-current">{{ drilledRoot.name }}</span>
              </NBreadcrumbItem>
            </NBreadcrumb>
          </div>
        </template>
        <NEmpty v-if="categoryBarsData.length === 0" :description="t('reports.category.empty')" />
        <div
          v-else
          data-testid="category-chart-scroll"
          style="max-height: 320px; overflow-y: auto"
        >
          <div
            data-testid="category-chart-canvas"
            :style="{ height: `${categoryChartHeight}px`, position: 'relative' }"
          >
            <Bar
              class="category-chart"
              :data="categoryChartData"
              :options="categoryChartOptions"
              :plugins="[barEndLabelPlugin]"
            />
          </div>
        </div>
      </NCard>
      <MerchantRankingPanel :shares="merchantShares" />
    </NSpace>
  </NSpin>
</template>

<style scoped>
/* 分类卡头部：标题与下钻面包屑同行（issue #379） */
.category-card-header {
  display: flex;
  align-items: center;
  gap: 12px;
}
</style>
