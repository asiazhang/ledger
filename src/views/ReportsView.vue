<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { NCard, NSpace, NEmpty, NSpin, NBreadcrumb, NBreadcrumbItem } from 'naive-ui'
import QuickTimeRange from '@/components/QuickTimeRange.vue'
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
import { useAppStore } from '@/stores/app'
import { useReportsSessionStore } from '@/stores/reports-session'
import { kindSemanticColor } from '@/theme/semantic-colors'
import { formatAmount } from '@/types'
import type { CategoryShare, MerchantShare, MonthlySummary } from '@/types'
import {
  barEndLabel,
  categoryBarTotal,
  categoryBars,
  categoryDrilldownBars,
} from '@/utils/category-chart'
import { categoryRoot } from '@/utils/category-tree'
import { UNCATEGORIZED_ONLY } from '@/composables/useTransactionFilter'
import {
  DATED_TIME_PERIOD_PRESETS,
  type NullableDateRange,
} from '@/utils/time-period'
import MerchantRankingPanel from '@/components/reports/MerchantRankingPanel.vue'

ChartJS.register(Title, Tooltip, Legend, BarElement, CategoryScale, LinearScale)

const reference = useReferenceStore()
const router = useRouter()
// 报表页会话状态（issue #427）：期间快照与图内下钻提升为会话级 store——
// 同一会话内离开报表页再回来（Cmd+左回退、侧栏切换）= 回到离开时的样子；
// 冷启动回默认「当年」；不写 localStorage、不写回路由 URL。
// 同值守卫与期间切换复位下钻两条规则内化在 store，视图只接线。
const session = useReportsSessionStore()
// 月度收支图三根语义色柱随主题响应式换色（issue #435）：色值单一来源在
// @/theme/semantic-colors，与交易列表/搜索金额列同源；barChartData 读
// app store 主题，切换外观即时重算，无需重建图表。
const app = useAppStore()

// 报表页日期闭集（ADR-0057）：仅四枚日期芯片、无「全部」——期间必有界。
const REPORT_PRESETS = DATED_TIME_PERIOD_PRESETS

// 共享受控组件受控桥接（issue #410）：快照区间进出，组件不持状态源，
// 唯一事实源是会话状态 store（issue #427）。四枚芯片与步进/面板只产出双端
// 有界的自然周期快照，单端缺失（NullableDateRange 契约允许的过渡态）不采纳、
// 期间保持有界；同值守卫在 store 内化（重复点同一芯片不重拉、不复位下钻）。
const quickRange = computed<NullableDateRange>({
  get: () => ({ from: session.period.from, to: session.period.to }),
  set: (range) => {
    if (range.from !== null && range.to !== null) {
      session.setPeriod({ from: range.from, to: range.to })
    }
  },
})

const monthly = ref<MonthlySummary[]>([])
const shares = ref<CategoryShare[]>([])
const merchantShares = ref<MerchantShare[]>([])
const loading = ref(false)

async function refresh() {
  loading.value = true
  try {
    const [m, s, ms] = await Promise.all([
      // 三张卡随所选期间重算（issue #411）：期间口径一致，聚合在后端收口；
      // 期间读自会话状态 store（issue #427），恢复/改选同规
      api.monthlySummary({ from: session.period.from, to: session.period.to }),
      api.categoryShares('expense', { from: session.period.from, to: session.period.to }),
      api.merchantShares({ from: session.period.from, to: session.period.to }),
    ])
    monthly.value = m
    shares.value = s
    merchantShares.value = ms
  } finally {
    loading.value = false
  }
}

watch(
  // 监听原始值元组而非对象引用：只要期间双端任一变化就重拉，
  // 不依赖 store 内部「快照整体替换」的实现方式（issue #427）
  () => [session.period.from, session.period.to] as const,
  () => {
    // 期间切换复位下钻已内化在 store.setPeriod（issue #427）；视图只负责
    // 照常重拉：三卡按当前期间重算，离开期间新记的账进入即反映
    refresh()
  },
)

const barChartData = computed(() => ({
  labels: monthly.value.map((m) => m.month),
  // dataset 顺序固定：0=收入、1=支出、2=退款；tooltip 回调按 datasetIndex 取值，
  // 不依赖 label 字符串（label 已随界面语言迁移，不能再当判断依据）。
  // 三柱颜色与交易列表同类金额同源（issue #435）：语义色模块按当前主题取值。
  datasets: [
    { label: t('reports.monthly.income'), data: monthly.value.map((m) => m.income_cents), backgroundColor: kindSemanticColor('income', app.theme) },
    { label: t('reports.monthly.expense'), data: monthly.value.map((m) => m.expense_cents), backgroundColor: kindSemanticColor('expense', app.theme) },
    { label: t('reports.monthly.refund'), data: monthly.value.map((m) => m.refund_cents), backgroundColor: kindSemanticColor('refund', app.theme) },
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
  session.drilledRootId
    ? (reference.categories.find((c) => c.id === session.drilledRootId) ?? null)
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

/** 跳转下钻（issue #380，载荷期间化 issue #412）：直达按该分类过滤的交易列表。
 * 载荷 = 分类（保留值 none = 仅无分类）+ 所选期间首尾日期——期间本就是共享受控
 * 组件经时间周期纯函数（presetRange / periodRange）写回会话状态 store 的精确
 * 自然周期快照，月/季/年各档边界同源复用、不在视图另搓第二份年界数学；刻意不带
 * 交易类型参数——退款继承原分类，列表净额与图中柱值一致（分类下钻词条
 * 「跳转载荷与图所见同口径」）。 */
function goCategoryTransactions(categoryId: string) {
  router.push({
    name: 'transactions',
    query: {
      category: categoryId,
      dateFrom: session.period.from,
      dateTo: session.period.to,
    },
  })
}

/** 点柱分派（issue #379 图内下钻 + #380 跳转下钻）：两段式的完整接线。
 * 基础态：一级分类 → 图内下钻（参考数据可解析守卫：下钻分类中途被删时点击不卡死）；
 * 未分类柱（id 为 null）→ 直达「仅无分类」列表（未分类无子结构，是柱不是层级）。
 * 下钻态：二级子分类行与父直挂行都直达该分类列表——直挂行 id 即父分类 id，
 * 同一载荷构造天然覆盖「按父分类精确过滤」。 */
function handleCategoryBarClick(_event: unknown, elements: ActiveElement[]) {
  const bar = categoryBarsData.value[elements[0]?.index ?? -1]
  if (!bar) return
  if (!drilledRoot.value) {
    if (!bar.id) {
      goCategoryTransactions(UNCATEGORIZED_ONLY)
      return
    }
    if (!categoryRoot(reference.categories, bar.id)) return
    session.setDrilldown(bar.id)
    return
  }
  // 下钻行 id 非空由下钻纯函数保证（CategoryBar.id 类型联合故此处收窄）
  if (!bar.id) return
  goCategoryTransactions(bar.id)
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

onMounted(() => {
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll；
  // 数据期间边界由 QuickTimeRange 组件内化（issue #410），视图不再自拉
  void refresh()
})
</script>

<template>
  <NSpin :show="loading">
    <NSpace vertical :size="16">
      <!-- 报表页唯一时间控件（issue #411）：四枚日期芯片（无「全部」）＋期间步进器＋
           期间直达面板，钳制与「今天」时钟由组件内化；年份下拉已退役（ADR-0057） -->
      <QuickTimeRange v-model="quickRange" :presets="REPORT_PRESETS" />
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
              <NBreadcrumbItem @click="session.setDrilldown(null)">
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
