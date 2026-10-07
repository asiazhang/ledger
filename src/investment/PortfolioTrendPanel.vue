<script setup lang="ts">
import { computed } from "vue";
import { NEmpty, NRadio, NRadioGroup, NSpace, NSpin, NText } from "naive-ui";
import { useReferenceStore } from "@/stores/reference";
import ConceptLabel from "@/investment/ConceptLabel.vue";
import { formatAmount, amountPrivacyEnabled } from "@ledger/money";
import { t } from "@ledger/i18n";
import { Line } from "vue-chartjs";
import type { ChartOptions, TooltipItem } from "chart.js";
// Chart.js 统一注册模块（issue #926）：折线图所需 controller/element/scale 一处
// 注册，不再组件自持子集（缺项曾致渲染错误循环冻结界面）；导入即完成注册。
import "@ledger/utils/chart-registration";
import { TREND_RANGE_PRESETS, usePortfolioTrend } from "@/investment/usePortfolioTrend";

const reference = useReferenceStore();
// 走势的区间选择住投资页会话状态 store（issue #1192，ADR-0094 会话内保留）：
// 页签重挂后仍是离开时的那个区间。#1907 起单标的模式随走势页签退役，
// 面板收缩为组合市值曲线一维。
const trend = usePortfolioTrend();

/** 有通道无历史数据的引导文案：按补全状态三态（ADR-0122 决策 5 / issue #1377）
 * ——不再有指向「同步标的信息」的回填文案（同步只刷现价，历史由后台补全）。 */
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
  // 无补全字段（区间裁剪 / 持仓与价格周错开等）：既有空态文案的中性改写。
  return t("investments.trend.emptyNoPointsInRange");
});

const currency = computed(() =>
  trend.currencyCode.value ? reference.currencyMap.get(trend.currencyCode.value) : undefined,
);

/** 币种口径标注：组合走势 = 本位币 */
const currencyCaption = computed(() =>
  trend.currencyCode.value
    ? t("investments.trend.captionPortfolio", { currency: trend.currencyCode.value })
    : "",
);

/** 曲线值格式化（金额分走 formatAmount，随隐私开关掩码） */
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
      // 停牌/缺价周：x 轴按日期连续、缺口连点跨越（ADR-0019）
      spanGaps: true,
    },
  ],
}));

// options computed 并读取隐私开关建立响应式依赖（issue #566）：tooltip 与轴刻度 formatter
// 已同源走 formatAmount，但只在重绘时执行——切换时靠 options 变更驱动
// vue-chartjs 重绘，满足「切换即时生效于所有已打开页面」（spec #564 user story 14）。
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
