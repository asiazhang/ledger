<script setup lang="ts">
import { computed } from 'vue'
import { NCard, NEmpty, NRadioButton, NRadioGroup } from 'naive-ui'
import { Bar } from 'vue-chartjs'
import {
  Chart as ChartJS,
  Tooltip,
  BarElement,
  CategoryScale,
  LinearScale,
} from 'chart.js'
import type { ActiveElement, ChartOptions, TooltipItem } from 'chart.js'
import { t } from '@/i18n'
import { useAppStore } from '@/stores/app'
import { MERCHANT_TOP_N_OPTIONS } from '@/stores/reports-session'
import {
  SOFT_BAR_RADIUS,
  SOFT_TOOLTIP,
  barEndAmountPlugin,
  softBarFillPlugin,
  softChartColors,
} from '@/theme/chart-style'
import { formatAmount } from '@/types'
import { amountPrivacyEnabled } from '@/utils/money'
import { barTooltipLabel } from '@/utils/category-chart'
import { merchantBars } from '@/utils/merchant-chart'
import type { MerchantSharesReport } from '@/types'

ChartJS.register(Tooltip, BarElement, CategoryScale, LinearScale)

// 商户消费排行面板（issue #192 → #588 柱图化）：支出分类构成同款横向柱状图。
// 口径、排序与 topN 截断全部在后端 `merchant_shares` 收口，前端按返回序渲染
// 零口径逻辑；柱色与分类构成同源（分类色板按名次序，多颜色 + hex 实色，
// 渐隐渐变由 softBarFillPlugin 绘制期呈现）；tooltip =
// 名称（类目轴）+ 金额 · 占比%（复用 barTooltipLabel，分母 = 后端载荷的全量合计，
// 不是展示中的前 N 行合计）。
//
// 点击下钻（issue #589）：点任意商户柱 → 上报 drilldown 事件（携带 merchant_id），
// 由父组件（报表视图）构造跳转载荷（merchant + 日期边界 + 收支类型集合）。
// 面板保持受控不持状态源：跳转逻辑不在本组件，与分类下钻的「载荷由视图显式构造」
// 同一接缝，本组件只做把「点了哪根柱」这个意图上报出去。
//
// TopN 控件：卡片头部 Top 5 / Top 10 两枚选项（档位闭集二，默认 5），选择归
// 报表页会话 store（会话内保留、冷启动回默认，ADR-0061 同粒度）；本组件受控
// 不持状态源，v-model:topN 进出。
const props = defineProps<{ report: MerchantSharesReport; topN: number }>()
const emit = defineEmits<{
  (e: 'update:topN', value: number): void
  (e: 'drilldown', merchantId: string): void
}>()

const app = useAppStore()

const bars = computed(() => merchantBars(props.report.rows))

/** 点柱上报（issue #589）：按点击索引取对应商户柱，携带 merchant_id 上报下钻意图。
 *  柱行 merchant_id 非空由 merchantBars 保证（MerchantShare.merchant_id 同源透传），
 *  此处置索引越界防护（稳妥不越界）；无商户交易不进排行、无下钻入口（后端已排除
 *  无商户行，本组件零口径）。 */
function handleClick(_event: unknown, elements: ActiveElement[]) {
  const bar = bars.value[elements[0]?.index ?? -1]
  if (!bar) return
  emit('drilldown', bar.merchant_id)
}

const chartData = computed(() => ({
  labels: bars.value.map((b) => b.name),
  datasets: [
    {
      data: bars.value.map((b) => b.value),
      backgroundColor: bars.value.map((b) => b.color),
      barThickness: 20,
      borderRadius: SOFT_BAR_RADIUS,
    },
  ],
}))

// 视觉柔化与分类图同源（chart-style 单一来源）；options computed 并读取隐私开关
// 建立响应式依赖（issue #566 同款）：轴刻度/tooltip 只在重绘时执行，切换隐私
// 靠 options 变更驱动 vue-chartjs 重绘，即时生效。
const chartOptions = computed<ChartOptions<'bar'>>(() => {
  void amountPrivacyEnabled.value
  const soft = softChartColors(app.theme)
  return {
    indexAxis: 'y',
    color: soft.ticks,
    responsive: true,
    maintainAspectRatio: false,
    // 两端留白容柱尾标签（x 轴 grace 把最大/最小值两端各拓 30%）
    layout: { padding: { left: 4, right: 8 } },
    onClick: handleClick,
    plugins: {
      legend: { display: false },
      tooltip: {
        ...SOFT_TOOLTIP,
        callbacks: {
          // 名称在 tooltip 标题（类目轴标签默认值）；label = 金额 · 占比%，
          // 占比分母 = 后端全量合计（issue #588：全部商户而不仅是展示中的前 N）
          label: (context: TooltipItem<'bar'>) =>
            barTooltipLabel(context.raw as number, props.report.total_cents),
        },
      },
    },
    scales: {
      x: {
        type: 'linear',
        grace: '30%',
        grid: { color: soft.grid },
        border: { display: false },
        ticks: {
          color: soft.ticks,
          callback: (value: number | string) => formatAmount(Number(value)),
        },
      },
      y: {
        grid: { display: false },
        border: { display: false },
        // 平铺不截断：全部类目标签都画，行多时容器滚动
        ticks: { autoSkip: false, color: soft.ticks },
      },
    },
  }
})

// 平铺滚动（分类图同款）：图高随行数增长，卡片内限高滚动。
const MERCHANT_ROW_HEIGHT = 32
const MERCHANT_MIN_ROWS = 6
const chartHeight = computed(
  () => Math.max(MERCHANT_MIN_ROWS, bars.value.length) * MERCHANT_ROW_HEIGHT,
)
</script>

<template>
  <NCard size="small">
    <template #header>
      <div class="merchant-card-header">
        <span>{{ t('reports.merchant.title') }}</span>
        <!-- TopN 档位（issue #588）：Top 5 / Top 10 两枚选项，受控进出会话 store -->
        <NRadioGroup
          :value="topN"
          size="small"
          data-testid="merchant-topn"
          @update:value="(v: number) => emit('update:topN', v)"
        >
          <NRadioButton
            v-for="n in MERCHANT_TOP_N_OPTIONS"
            :key="n"
            :value="n"
            :data-testid="`merchant-topn-${n}`"
          >
            {{ t('reports.merchant.topOption', { n }) }}
          </NRadioButton>
        </NRadioGroup>
      </div>
    </template>
    <NEmpty
      v-if="report.rows.length === 0"
      :description="t('reports.merchant.empty')"
      data-testid="merchant-empty"
    />
    <div
      v-else
      data-testid="merchant-chart-scroll"
      style="max-height: 320px; overflow-y: auto"
    >
      <div
        data-testid="merchant-chart-canvas"
        :style="{ height: `${chartHeight}px`, position: 'relative' }"
      >
        <Bar
          class="merchant-chart"
          :data="chartData"
          :options="chartOptions"
          :plugins="[barEndAmountPlugin, softBarFillPlugin]"
        />
      </div>
    </div>
  </NCard>
</template>

<style scoped>
/* 商户卡头部：标题与 TopN 档位控件同行（分类卡头部面包屑同构） */
.merchant-card-header {
  display: flex;
  align-items: center;
  gap: 12px;
}
</style>
