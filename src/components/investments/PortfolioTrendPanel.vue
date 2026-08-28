<script setup lang="ts">
import { computed, watch } from 'vue'
import { NEmpty, NRadio, NRadioGroup, NSelect, NSpace, NSpin, NText } from 'naive-ui'
import { Line } from 'vue-chartjs'
import { Chart as ChartJS, Tooltip, Legend, CategoryScale, LinearScale } from 'chart.js'
import type { ChartOptions, TooltipItem } from 'chart.js'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount } from '@/types'
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
    ? `单位：${trend.currencyCode.value}（本位币）`
    : `计价币种：${trend.currencyCode.value}`
})

const datasetLabel = computed(() => {
  if (trend.mode.value === 'portfolio') return '组合市值'
  const inst = trend.instrument.value
  return inst ? `${inst.symbol} ${inst.name ?? ''}`.trim() : '标的'
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

const chartOptions: ChartOptions<'line'> = {
  responsive: true,
  maintainAspectRatio: false,
  interaction: { mode: 'nearest', intersect: false },
  plugins: {
    legend: { position: 'top' },
    tooltip: {
      callbacks: {
        label: (context: TooltipItem<'line'>) =>
          `${context.dataset.label}: ${formatAmount(context.raw as number, currency.value)}`,
      },
    },
  },
  scales: {
    x: {
      ticks: { maxRotation: 0, autoSkip: true, maxTicksLimit: 12 },
    },
    y: {
      ticks: {
        callback: (value: number | string) => formatAmount(Number(value), currency.value),
      },
    },
  },
}

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
        <NRadio value="portfolio">组合市值</NRadio>
        <NRadio value="instrument">单标的</NRadio>
      </NRadioGroup>
      <NSelect
        v-if="trend.mode.value === 'instrument'"
        v-model:value="selectedInstrumentId"
        :options="instrumentOptions"
        placeholder="选择标的..."
        filterable
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
          {{ p.label }}
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
        description="暂无行情来源"
        size="large"
      >
        <template #extra>
          <NText depth="3">
            该标的类型暂不参与行情采集（仅股票 / ETF 支持），不提供走势图。
          </NText>
        </template>
      </NEmpty>

      <!-- 无历史数据：引导去「同步持仓价格」回填近两年周线（ADR-0019 单通道） -->
      <NEmpty
        v-else-if="trend.isEmpty.value"
        data-testid="trend-empty"
        description="暂无历史价格数据"
        size="large"
      >
        <template #extra>
          <NText depth="3">
            点击标的列表的「同步持仓价格」回填近两年行情后，即可查看走势。
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
