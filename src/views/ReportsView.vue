<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { NButton, NCard, NSpace, NEmpty, NSpin, NBreadcrumb, NBreadcrumbItem } from "naive-ui";
import QuickTimeRange from "@/components/QuickTimeRange.vue";
import { useInputMode } from "@/composables/useInputMode";
import { useLoadable } from "@ledger/loadable";
import { Bar } from "vue-chartjs";
import type { ActiveElement, ChartOptions, TooltipItem } from "chart.js";
import "@ledger/utils/chart-registration";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import { useAppStore } from "@/stores/app";
import { useReportsSessionStore } from "@/reports/reports-session";
import { registerViewReset } from "@/composables/viewResetRegistry";
import { kindSemanticColor } from "@ledger/theme/semantic-colors";
import {
  SOFT_BAR_PERCENTAGE,
  SOFT_BAR_RADIUS,
  SOFT_CATEGORY_PERCENTAGE,
  SOFT_LEGEND_LABELS,
  SOFT_TOOLTIP,
  barEndAmountPlugin,
  softBarFillPlugin,
  softChartColors,
} from "@ledger/theme/chart-style";
import { formatAmount, amountPrivacyEnabled } from "@ledger/money";
import type { CategoryShare, MerchantSharesReport, MonthlySummary } from "@ledger/types";
import {
  barTooltipLabel,
  categoryBarTotal,
  categoryBars,
  categoryDrilldownBars,
} from "@ledger/utils/category-chart";
import { categoryRoot } from "@ledger/utils/category-tree";
import {
  UNCATEGORIZED_ONLY,
  CATEGORY_DRILLDOWN_KINDS,
  MERCHANT_DRILLDOWN_KINDS,
} from "@/transaction/useTransactionFilter";
import { DATED_TIME_PERIOD_PRESETS, type NullableDateRange } from "@ledger/utils/time-period";
import MerchantRankingPanel from "@/reports/MerchantRankingPanel.vue";

const reference = useReferenceStore();
const router = useRouter();
const session = useReportsSessionStore();

registerViewReset(session.resetToDefault);
const app = useAppStore();

const REPORT_PRESETS = DATED_TIME_PERIOD_PRESETS;

const quickRange = computed<NullableDateRange>({
  get: () => ({ from: session.period.from, to: session.period.to }),
  set: (range) => {
    if (range.from !== null && range.to !== null) {
      session.setPeriod({ from: range.from, to: range.to });
    }
  },
});

const monthly = ref<MonthlySummary[]>([]);
const shares = ref<CategoryShare[]>([]);
const merchantReport = ref<MerchantSharesReport>({ rows: [], total_cents: 0 });

const { loading, run: runMainRefresh } = useLoadable(async () => {
  const [monthlySummary, categoryShares] = await Promise.all([
    api.monthlySummary({ from: session.period.from, to: session.period.to }),
    api.categoryShares("expense", { from: session.period.from, to: session.period.to }),
  ]);
  return { monthlySummary, categoryShares };
});

async function refresh() {
  const result = await runMainRefresh();
  if (result === null) return;
  monthly.value = result.monthlySummary;
  shares.value = result.categoryShares;
}

const { run: runMerchantRefresh } = useLoadable(() =>
  api.merchantShares({ from: session.period.from, to: session.period.to }, session.merchantTopN),
);

async function fetchMerchantReport() {
  const report = await runMerchantRefresh();
  if (report !== null) merchantReport.value = report;
}

watch(
  () => session.merchantTopN,
  () => {
    void fetchMerchantReport();
  },
);

watch(
  () => [session.period.from, session.period.to] as const,
  () => {
    void refresh();
    void fetchMerchantReport();
  },
);

const barChartData = computed(() => ({
  labels: monthly.value.map((m) => m.month),
  datasets: [
    {
      label: t("reports.monthly.income"),
      data: monthly.value.map((m) => m.income_cents),
      backgroundColor: kindSemanticColor("income", app.theme),
      borderRadius: SOFT_BAR_RADIUS,
      barPercentage: SOFT_BAR_PERCENTAGE,
      categoryPercentage: SOFT_CATEGORY_PERCENTAGE,
    },
    {
      label: t("reports.monthly.expense"),
      data: monthly.value.map((m) => m.expense_cents),
      backgroundColor: kindSemanticColor("expense", app.theme),
      borderRadius: SOFT_BAR_RADIUS,
      barPercentage: SOFT_BAR_PERCENTAGE,
      categoryPercentage: SOFT_CATEGORY_PERCENTAGE,
    },
    {
      label: t("reports.monthly.refund"),
      data: monthly.value.map((m) => m.refund_cents),
      backgroundColor: kindSemanticColor("refund", app.theme),
      borderRadius: SOFT_BAR_RADIUS,
      barPercentage: SOFT_BAR_PERCENTAGE,
      categoryPercentage: SOFT_CATEGORY_PERCENTAGE,
    },
  ],
}));

const barChartOptions = computed<ChartOptions<"bar">>(() => {
  void amountPrivacyEnabled.value;
  const soft = softChartColors(app.theme);
  return {
    color: soft.ticks,
    responsive: true,
    maintainAspectRatio: false,
    plugins: {
      legend: { position: "top", labels: { ...SOFT_LEGEND_LABELS } },
      tooltip: {
        ...SOFT_TOOLTIP,
        callbacks: {
          label: (context: TooltipItem<"bar">) =>
            `${context.dataset.label}: ${formatAmount(context.raw as number)}`,
          afterBody: (items: TooltipItem<"bar">[]) => {
            let income = 0;
            let expense = 0;
            let refund = 0;
            for (const item of items) {
              if (item.datasetIndex === 0) income = item.raw as number;
              if (item.datasetIndex === 1) expense = item.raw as number;
              if (item.datasetIndex === 2) refund = item.raw as number;
            }
            const net = income - expense + refund;
            return t("reports.monthly.net", { amount: formatAmount(net) });
          },
        },
      },
    },
    scales: {
      x: {
        grid: { display: false },
        border: { display: false },
        ticks: { color: soft.ticks },
      },
      y: {
        grid: { color: soft.grid },
        border: { display: false },
        ticks: {
          color: soft.ticks,
          callback: (value: number | string) => formatAmount(Number(value)),
        },
      },
    },
  };
});

const drilledRoot = computed(() =>
  session.drilledRootId
    ? (reference.categories.find((c) => c.id === session.drilledRootId) ?? null)
    : null,
);

const categoryBarsData = computed(() => {
  const root = drilledRoot.value;
  if (root) {
    return categoryDrilldownBars(
      shares.value,
      reference.categories,
      root.id,
      t("reports.category.direct", { name: root.name }),
    );
  }
  return categoryBars(shares.value, reference.categories);
});

function goCategoryTransactions(categoryId: string) {
  router.push({
    name: "transactions",
    query: {
      category: categoryId,
      dateFrom: session.period.from,
      dateTo: session.period.to,
      kinds: CATEGORY_DRILLDOWN_KINDS,
    },
  });
}

function handleCategoryBarClick(_event: unknown, elements: ActiveElement[]) {
  const bar = categoryBarsData.value[elements[0]?.index ?? -1];
  if (!bar) return;
  if (!drilledRoot.value) {
    if (!bar.id) {
      goCategoryTransactions(UNCATEGORIZED_ONLY);
      return;
    }
    if (!categoryRoot(reference.categories, bar.id)) return;
    session.setDrilldown(bar.id);
    return;
  }
  if (!bar.id) return;
  goCategoryTransactions(bar.id);
}

function goMerchantTransactions(merchantId: string) {
  router.push({
    name: "transactions",
    query: {
      merchant: merchantId,
      dateFrom: session.period.from,
      dateTo: session.period.to,
      kinds: MERCHANT_DRILLDOWN_KINDS,
    },
  });
}

const categoryChartData = computed(() => ({
  labels: categoryBarsData.value.map((b) => b.name),
  datasets: [
    {
      data: categoryBarsData.value.map((b) => b.value),
      backgroundColor: categoryBarsData.value.map((b) => b.color),
      barThickness: 20,
      borderRadius: SOFT_BAR_RADIUS,
    },
  ],
}));

const CATEGORY_ROW_HEIGHT = 32;
const CATEGORY_MIN_ROWS = 6;
const categoryChartHeight = computed(() => {
  const rows = Math.max(CATEGORY_MIN_ROWS, categoryBarsData.value.length);
  return rows * CATEGORY_ROW_HEIGHT;
});

const inputMode = useInputMode();
const isTouch = computed(() => inputMode.value === "touch");
const showPercent = ref(false);
const categoryBarEndLabels = computed(() =>
  isTouch.value && showPercent.value
    ? categoryBarsData.value.map((b) =>
        barTooltipLabel(b.value, categoryBarTotal(categoryBarsData.value)),
      )
    : undefined,
);

const categoryChartOptions = computed<ChartOptions<"bar">>(() => {
  void amountPrivacyEnabled.value;
  const soft = softChartColors(app.theme);
  return {
    indexAxis: "y",
    color: soft.ticks,
    responsive: true,
    maintainAspectRatio: false,
    layout: { padding: { left: 4, right: 8 } },
    onClick: handleCategoryBarClick,
    plugins: {
      legend: { display: false },
      barEndAmounts: { labels: categoryBarEndLabels.value },
      tooltip: {
        ...SOFT_TOOLTIP,
        callbacks: {
          label: (context: TooltipItem<"bar">) =>
            barTooltipLabel(context.raw as number, categoryBarTotal(categoryBarsData.value)),
        },
      },
    },
    scales: {
      x: {
        type: "linear",
        grace: "30%",
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
        ticks: { autoSkip: false, color: soft.ticks },
      },
    },
  };
});

onMounted(() => {
  void refresh();
  void fetchMerchantReport();
});
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
          <Bar :data="barChartData" :options="barChartOptions" :plugins="[softBarFillPlugin]" />
        </div>
      </NCard>
      <NCard size="small">
        <template #header>
          <div class="category-card-header">
            <span>{{ t("reports.category.title") }}</span>
            <!-- 面包屑（issue #379）：下钻态显示当前位置，点根返回基础态 -->
            <NBreadcrumb v-if="drilledRoot" data-testid="category-breadcrumb" separator="›">
              <NBreadcrumbItem @click="session.setDrilldown(null)">
                <span data-testid="breadcrumb-root">{{ t("reports.category.all") }}</span>
              </NBreadcrumbItem>
              <NBreadcrumbItem>
                <span data-testid="breadcrumb-current">{{ drilledRoot.name }}</span>
              </NBreadcrumbItem>
            </NBreadcrumb>
            <!-- 触控轴占比一击可达（issue #843）：点按切换柱尾「金额 · 占比%」标注 -->
            <NButton
              v-if="isTouch"
              size="tiny"
              quaternary
              data-testid="category-percent-toggle"
              :type="showPercent ? 'primary' : 'default'"
              :aria-pressed="showPercent"
              class="touch-hit-area"
              @click="showPercent = !showPercent"
            >
              {{ t("reports.category.showPercent") }}
            </NButton>
          </div>
        </template>
        <NEmpty v-if="categoryBarsData.length === 0" :description="t('reports.category.empty')" />
        <div v-else data-testid="category-chart-scroll" style="max-height: 320px; overflow-y: auto">
          <div
            data-testid="category-chart-canvas"
            :style="{ height: `${categoryChartHeight}px`, position: 'relative' }"
          >
            <Bar
              class="category-chart"
              :data="categoryChartData"
              :options="categoryChartOptions"
              :plugins="[barEndAmountPlugin, softBarFillPlugin]"
            />
          </div>
        </div>
      </NCard>
      <MerchantRankingPanel
        :report="merchantReport"
        :top-n="session.merchantTopN"
        @update:top-n="session.setMerchantTopN"
        @drilldown="goMerchantTransactions"
      />
    </NSpace>
  </NSpin>
</template>

<style scoped>
.category-card-header {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
}
.category-card-header :deep(.n-button) {
  margin-left: auto;
  --touch-hit-inset: -12px -14px;
}
</style>
