<script setup lang="ts">
import { computed } from "vue";
import { NEmpty, NRadio, NRadioGroup, NSpace, NSpin, NText } from "naive-ui";
import { useReferenceStore } from "@/stores/reference";
import ConceptLabel from "@/investment/ConceptLabel.vue";
import { formatAmount, amountPrivacyEnabled } from "@ledger/money";
import { t } from "@ledger/i18n";
import { Line } from "vue-chartjs";
import type { ChartOptions, TooltipItem } from "chart.js";
import "@ledger/utils/chart-registration";
import { TREND_RANGE_PRESETS, usePortfolioTrend } from "@/investment/usePortfolioTrend";

const reference = useReferenceStore();
const trend = usePortfolioTrend();

const emptyExtra = computed(() => {
  const state = trend.backfill.value?.state;
  if (state === "running") {
    const { done, total } = trend.backfill.value!;
    return total != null
      ? t("investments.trend.emptyBackfillProgress", { done: done ?? 0, total })
      : t("investments.trend.emptyBackfillRunning");
  }
  if (state === "retry_pending") return t("investments.trend.emptyBackfillRetryPending");
  if (state === "no_data") return t("investments.trend.emptyBackfillNoData");
  return t("investments.trend.emptyNoPointsInRange");
});

const currency = computed(() =>
  trend.currencyCode.value ? reference.currencyMap.get(trend.currencyCode.value) : undefined,
);

const currencyCaption = computed(() =>
  trend.currencyCode.value
    ? t("investments.trend.captionPortfolio", { currency: trend.currencyCode.value })
    : "",
);

function formatTrendValue(value: number): string {
  return formatAmount(value, currency.value);
}

const datasetLabel = computed(() => t("investments.trend.modePortfolio"));

const chartData = computed(() => ({
  labels: trend.chartSeries.value.labels,
  datasets: [
    {
      label: datasetLabel.value,
      data: trend.chartSeries.value.values,
      borderColor: "#2080f0",
      backgroundColor: "rgba(32, 128, 240, 0.15)",
      tension: 0.25,
      pointRadius: 2,
      spanGaps: true,
    },
  ],
}));

const chartOptions = computed<ChartOptions<"line">>(() => {
  void amountPrivacyEnabled.value;
  return {
    responsive: true,
    maintainAspectRatio: false,
    interaction: { mode: "nearest", intersect: false },
    plugins: {
      legend: { position: "top" },
      tooltip: {
        callbacks: {
          label: (context: TooltipItem<"line">) =>
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
  };
});
</script>

<template>
  <NSpace vertical :size="12">
    <NSpace align="center" :size="16">
      <NRadioGroup
        :value="trend.preset.value"
        size="small"
        data-testid="trend-range"
        @update:value="trend.setPreset"
      >
        <NRadio v-for="p in TREND_RANGE_PRESETS" :key="p.value" :value="p.value">
          {{ t(p.labelKey) }}
        </NRadio>
      </NRadioGroup>
    </NSpace>

    <!-- 曲线口径说明（issue #1369）：常驻 ⓘ 回答「这条线画的是什么」——与币种
         标注同排，不随空态消失。 -->
    <NSpace align="center" :size="8">
      <ConceptLabel
        :label="t('investments.trend.modePortfolio')"
        concept="portfolioTrend"
        test-id="trend-concept"
      />
      <NText v-if="currencyCaption" depth="3" data-testid="trend-currency">
        {{ currencyCaption }}
      </NText>
    </NSpace>

    <NSpin :show="trend.loading.value">
      <!-- 有通道无历史数据：按补全状态三态给答案（补全中带计数 / 待重试 / 无数据），
           不指向同步按钮（issue #1377）。 -->
      <NEmpty
        v-if="trend.isEmpty.value"
        data-testid="trend-empty"
        :description="t('investments.trend.empty')"
        size="large"
      >
        <template #extra>
          <NText
            depth="3"
            :data-testid="trend.backfill.value ? 'trend-empty-backfill' : 'trend-empty-neutral'"
          >
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
