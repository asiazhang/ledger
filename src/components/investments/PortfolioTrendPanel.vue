<script setup lang="ts">
import { computed, watch } from 'vue'
import { NEmpty, NRadio, NRadioGroup, NSpace, NSpin, NText } from 'naive-ui'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { Line } from 'vue-chartjs'
import { Chart as ChartJS, Tooltip, Legend, CategoryScale, LinearScale } from 'chart.js'
import type { ChartOptions, TooltipItem } from 'chart.js'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount, formatPrice } from '@/types'
import { amountPrivacyEnabled } from '@/utils/money'
import { t } from '@/i18n'
import {
  hasMarketSource,
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

/** 当前标的是否走行情采集通道（非股票/ETF、市场未知 → 边界说明而非空图） */
const noMarketSource = computed(
  () => trend.mode.value === 'instrument' && !!trend.instrument.value && !hasMarketSource(trend.instrument.value),
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

ChartJS.register(Tooltip, Legend, CategoryScale, LinearScale)
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
      <!-- 非股票标的：暂无行情来源（PriceHistory 不覆盖，ADR-0019），给说明而非空白报错 -->
      <NEmpty
        v-if="noMarketSource"
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

      <!-- 无历史数据：引导去「同步持仓价格」回填近两年周线（ADR-0019 单通道） -->
      <NEmpty
        v-else-if="trend.isEmpty.value"
        data-testid="trend-empty"
        :description="t('investments.trend.empty')"
        size="large"
      >
        <template #extra>
          <NText depth="3">
            {{ t('investments.trend.emptyExtra') }}
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
