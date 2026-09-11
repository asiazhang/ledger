<script setup lang="ts">
import { computed, watch } from 'vue'
import { NEmpty, NRadio, NRadioGroup, NSpace, NSpin, NText } from 'naive-ui'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { Line } from 'vue-chartjs'
import type { ChartOptions, TooltipItem } from 'chart.js'
// Chart.js 统一注册模块（issue #926）：折线图所需 controller/element/scale 一处
// 注册，不再组件自持子集（缺项曾致渲染错误循环冻结界面）；导入即完成注册。
import '@/utils/chart-registration'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount, formatPrice } from '@/types'
import { amountPrivacyEnabled } from '@/utils/money'
import { t } from '@/i18n'
import {
  TREND_RANGE_PRESETS,
  usePortfolioTrend,
} from '@/composables/usePortfolioTrend'
import type { Instrument } from '@/types'

// 标的列表「走势」入口带入的标的（单标的模式起点）；面板内也可经下拉切换
const props = defineProps<{
  entryInstrument?: Instrument | null
}>()

const reference = useReferenceStore()
const trend = usePortfolioTrend()

watch(
  () => props.entryInstrument,
  (inst) => {
    if (inst) trend.showInstrument(inst)
  },
  { immediate: true },
)

const instrumentOptions = computed(() =>
  trend.instruments.value.map((i) => ({
    label: `${i.symbol} ${i.name ?? ''}`.trim(),
    value: i.id,
  })),
)

const selectedInstrumentId = computed({
  get: () => trend.instrument.value?.id ?? null,
  set: (id: string | null) => {
    const inst = trend.instruments.value.find((i) => i.id === id)
    if (inst) trend.showInstrument(inst)
  },
})

/** 无价格来源标的（后端判通道 = none，issue #1060）：边界说明而非空图。
 * 放行判定消费后端派生事实，前端不再按类型与市场自行推断。 */
const noPriceSource = computed(
  () =>
    trend.mode.value === 'instrument' &&
    trend.instrument.value?.price_channel === 'none',
)

/** 当前单标的的价格通道（组合模式为 null）；有通道无数据时按通道选引导文案 */
const priceChannel = computed(() =>
  trend.mode.value === 'instrument' ? trend.instrument.value?.price_channel ?? null : null,
)

/** 有通道无数据的引导文案：手动报价通道引导去「录价」，其余通道引导去同步 */
const emptyExtra = computed(() =>
  priceChannel.value === 'manual'
    ? t('investments.trend.emptyExtraManual')
    : t('investments.trend.emptyExtra'),
)

const currency = computed(() =>
  trend.currencyCode.value ? reference.currencyMap.get(trend.currencyCode.value) : undefined,
)

/** 币种口径标注：组合 = 本位币；单标的 = 报价币种 */
const currencyCaption = computed(() => {
  if (!trend.currencyCode.value) return ''
  return trend.mode.value === 'portfolio'
    ? t('investments.trend.captionPortfolio', { currency: trend.currencyCode.value })
    : t('investments.trend.captionInstrument', { currency: trend.currencyCode.value })
})

/**
 * 曲线值格式化（双刻度）：组合走势值为金额（分）走 formatAmount；单标的走势值为
 * 价格（万分之一元，ADR-0038 价格刻度）走 formatPrice。
 */
function formatTrendValue(value: number): string {
  const ccy = currency.value
  return trend.mode.value === 'portfolio'
    ? formatAmount(value, ccy)
    : formatPrice(value, ccy)
}

const datasetLabel = computed(() => {
  if (trend.mode.value === 'portfolio') return t('investments.trend.modePortfolio')
  const inst = trend.instrument.value
  return inst ? `${inst.symbol} ${inst.name ?? ''}`.trim() : t('investments.trend.instrumentFallback')
})

const chartData = computed(() => ({
  labels: trend.chartSeries.value.labels,
  datasets: [
    {
      label: datasetLabel.value,
      data: trend.chartSeries.value.values,
      borderColor: '#2080f0',
      backgroundColor: 'rgba(32, 128, 240, 0.15)',
      tension: 0.25,
      pointRadius: 2,
      // 停牌/缺价周：x 轴按日期连续、缺口连点跨越（ADR-0019）
      spanGaps: true,
    },
  ],
}))

// options computed 并读取隐私开关建立响应式依赖（issue #566）：tooltip 与轴刻度 formatter
// 已同源走 formatAmount/formatPrice，但只在重绘时执行——切换时靠 options 变更驱动
// vue-chartjs 重绘，满足「切换即时生效于所有已打开页面」（spec #564 user story 14）。
const chartOptions = computed<ChartOptions<'line'>>(() => {
  void amountPrivacyEnabled.value
  return {
    responsive: true,
    maintainAspectRatio: false,
    interaction: { mode: 'nearest', intersect: false },
    plugins: {
      legend: { position: 'top' },
      tooltip: {
        callbacks: {
          label: (context: TooltipItem<'line'>) =>
            `${context.dataset.label}: ${formatTrendValue(context.raw as number)}`,
        },
      },
    },
    scales: {
      x: {
        ticks: { maxRotation: 0, autoSkip: true, maxTicksLimit: 12 },
      },
      y: {
        ticks: {
          callback: (value: number | string) => formatTrendValue(Number(value)),
        },
      },
    },
  }
})

</script>

<template>
  <NSpace vertical :size="12">
    <NSpace align="center" :size="16">
      <NRadioGroup
        v-model:value="trend.mode.value"
        size="small"
        data-testid="trend-mode"
      >
        <NRadio value="portfolio">{{ t('investments.trend.modePortfolio') }}</NRadio>
        <NRadio value="instrument">{{ t('investments.trend.modeInstrument') }}</NRadio>
      </NRadioGroup>
      <PinyinSelect
        v-if="trend.mode.value === 'instrument'"
        v-model:value="selectedInstrumentId"
        :options="instrumentOptions"
        :placeholder="t('investments.trend.instrumentPlaceholder')"
        clearable
        style="width: 260px"
        data-testid="trend-instrument-select"
      />
      <NRadioGroup
        v-model:value="trend.preset.value"
        size="small"
        data-testid="trend-range"
      >
        <NRadio v-for="p in TREND_RANGE_PRESETS" :key="p.value" :value="p.value">
          {{ t(p.labelKey) }}
        </NRadio>
      </NRadioGroup>
      <NText v-if="currencyCaption" depth="3" data-testid="trend-currency">
        {{ currencyCaption }}
      </NText>
    </NSpace>

    <NSpin :show="trend.loading.value">
      <!-- 无价格来源标的（通道 = none，issue #1060）：说明「没有价格来源」而非空白报错 -->
      <NEmpty
        v-if="noPriceSource"
        data-testid="trend-no-source"
        :description="t('investments.trend.noSource')"
        size="large"
      >
        <template #extra>
          <NText depth="3">
            {{ t('investments.trend.noSourceExtra') }}
          </NText>
        </template>
      </NEmpty>

      <!-- 有通道无历史数据：按通道给可执行引导——净值/行情通道去同步，手动报价通道去录价 -->
      <NEmpty
        v-else-if="trend.isEmpty.value"
        data-testid="trend-empty"
        :description="t('investments.trend.empty')"
        size="large"
      >
        <template #extra>
          <NText depth="3">
            {{ emptyExtra }}
          </NText>
        </template>
      </NEmpty>

      <div v-else-if="trend.chartSeries.value.labels.length > 0" class="trend-chart-box">
        <Line :data="chartData" :options="chartOptions" />
      </div>
    </NSpin>
  </NSpace>
</template>

<style scoped>
.trend-chart-box {
  position: relative;
  height: 360px;
}
</style>
