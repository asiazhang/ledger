<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  NAlert,
  NButton,
  NCard,
  NEmpty,
  NGrid,
  NGridItem,
  NProgress,
  NSpace,
  NSpin,
  NTag,
  NText,
  NTooltip,
} from "naive-ui";
import { InformationCircleOutline } from "@vicons/ionicons5";
import { NIcon } from "naive-ui";
import { useRouter } from "vue-router";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { useDashboardOverview } from "@/dashboard/useDashboardOverview";
import { useFinancialFreedom } from "@/dashboard/useFinancialFreedom";
import { useInputMode } from "@/composables/useInputMode";
import { useWindowTier } from "@ledger/window-tier";
import { useReferenceStore } from "@/stores/reference";
import AppPopover from "@ledger/ui-kit/AppPopover.vue";
import ConceptTipHost from "@/investment/ConceptTipHost.vue";
import { formatAmount } from "@ledger/money";
import type { MonthlySummary } from "@ledger/types";
import { usePortfolioOverview } from "@/investment/usePortfolioOverview";
import PortfolioStatsCards from "@/investment/PortfolioStatsCards.vue";

// 首页财务全貌仪表盘（issue #140）：净资产总览卡（issue #143）+ 投资概览卡（issue #145）
// + 本月收支（issue #144）。
// 快速记账已迁至交易页「记一笔」弹窗、最近交易列表已移除（issue #141）；
// 逐账户余额卡已移除（逐账户明细归账户页，首页只呈现聚合全貌），
// 预算进度与物品使用成本两卡已移除（明细归预算页 / 物品页，issue #1950）；
// 投资概览卡不再按数据隐匿，空态用 NEmpty 占位。
const reference = useReferenceStore();
const currentMonth = ref<MonthlySummary | null>(null);

// 净资产总览卡（issue #143）：多币种折算与合计全部在后端 `dashboard_overview` 完成，
// 前端只做装配渲染；缺汇率等报错显示提示文案而非空数字。
const { overview, loading, error } = useDashboardOverview();

// 本月收支口径：收入用后端净收入列（income_net）；净支出为展示层计算
// 毛支出 − 退款（与预算消耗、分类占比的 expense_net 口径一致，退款不单列）；
// 结余 = 收入 − 净支出。当月无交易行时全部显示 0。
const netIncomeCents = computed(() => currentMonth.value?.income_cents ?? 0);
const netExpenseCents = computed(
  () => (currentMonth.value?.expense_cents ?? 0) - (currentMonth.value?.refund_cents ?? 0),
);
const balanceCents = computed(() => netIncomeCents.value - netExpenseCents.value);

// 投资概览卡（issue #145）：复用持仓概览 composable 的折本位币单值合计（issue #1797），
// 无任何持仓时整卡隐藏；无行情标的不以零计入合计（缺价行计数标注，缺汇率整卡警告）。
const {
  rows: holdingRows,
  statCards,
  cumulativePnl,
  nativeCurrency,
  refresh: refreshPortfolio,
} = usePortfolioOverview();

// 财务自由度卡（issue #344；口径 ADR-0048）：后端 financial_freedom 聚合
//（分子折算、分母年化、3% 提取率），前端只做装配渲染与阶段标签派生。
const router = useRouter();
const {
  data: freedom,
  loading: freedomLoading,
  error: freedomError,
  refresh: refreshFreedom,
} = useFinancialFreedom();

// 折算基准币种（分子/分母同币种展示用）
const freedomCurrency = computed(() =>
  freedom.value ? reference.getCurrency(freedom.value.native_currency) : undefined,
);

// 移动档适配（issue #847 / ADR-0088 决策 11 票⑦）：概览栅格单列化 + 悬停替代。
// 断点口径接窗口分级 composable 唯一事实源，不自立断点（响应栅格不用 naive
// 自带 640/1024 断点）；悬停替代按输入轴判定（平板横屏 = 桌面档 + 触控轴）。
const windowTier = useWindowTier();
const inputMode = useInputMode();
const isMobileTier = computed(() => windowTier.value === "mobile");
const isTouch = computed(() => inputMode.value === "touch");

/** 计算口径四行文案的 i18n key 闭集：tooltip 与触控气泡两轴共用同一数据源，
 * 文案只在一处声明。 */
const FREEDOM_INFO_KEYS = [
  "formula",
  "numeratorBreakdown",
  "denominatorBreakdown",
  "extractionRate",
] as const;

// 阶段标签前端派生（ADR-0048 决策 5：阈值与文案归 UI，不做后端枚举）：
// <30% 积累期 / 30–100% 接近自由 / ≥100% 财务自由；文案走 i18n（ADR-0049）
const freedomStage = computed(() => {
  if (!freedom.value) return null;
  if (freedom.value.ratio >= 100)
    return { label: t("dashboard.freedom.stageFree"), type: "success" as const };
  if (freedom.value.ratio >= 30)
    return { label: t("dashboard.freedom.stageApproaching"), type: "info" as const };
  return { label: t("dashboard.freedom.stageAccumulation"), type: "default" as const };
});

// 进度条：>100% 封顶展示，≥100% 转成功状态（达成时刻的视觉确认）
const freedomProgress = computed(() => {
  if (!freedom.value) return { percentage: 0, success: false };
  return {
    percentage: Math.min(100, freedom.value.ratio),
    success: freedom.value.ratio >= 100,
  };
});

function goBudget() {
  void router.push({ name: "budget" });
}

onMounted(async () => {
  const now = new Date();
  const year = now.getFullYear();
  const monthKey = `${year}-${String(now.getMonth() + 1).padStart(2, "0")}`;
  const monthly = await api.monthlySummary(year);
  currentMonth.value = monthly.find((m) => m.month === monthKey) ?? null;
});
</script>

<template>
  <NSpace vertical :size="16">
    <!-- 净资产总览卡（issue #143）：置顶呈现本位币单一主数字，第一眼回答「总共有多少钱」 -->
    <NCard size="small" data-testid="net-worth-card">
      <NSpin :show="loading">
        <NSpace vertical :size="4">
          <NSpace :size="4" align="center" :wrap="false">
            <NText depth="3" style="font-size: 12px">{{ t("dashboard.netWorth.label") }}</NText>
            <!-- 净资产口径说明（与财务自由度卡同款）：指针轴悬停即现、触控轴点按弹出，
                 文案单源 dashboard.netWorth.tooltip，两轴同一份。 -->
            <ConceptTipHost :text="t('dashboard.netWorth.tooltip')">
              <template #default="{ isTouch }">
                <NButton
                  text
                  :class="isTouch ? 'touch-hit-area' : undefined"
                  :style="isTouch ? { '--touch-hit-inset': '-10px -10px' } : undefined"
                  :aria-label="t('dashboard.netWorth.tooltipAria')"
                  data-testid="net-worth-info"
                >
                  <NIcon :size="14" color="var(--n-title-text-color, #999)">
                    <InformationCircleOutline />
                  </NIcon>
                </NButton>
              </template>
            </ConceptTipHost>
          </NSpace>
          <template v-if="overview">
            <NText strong style="font-size: 28px">
              {{
                formatAmount(
                  overview.net_worth_cents,
                  reference.getCurrency(overview.native_currency),
                )
              }}
            </NText>
          </template>
          <NAlert v-else-if="error" type="warning" :bordered="false">
            {{ error }}
          </NAlert>
        </NSpace>
      </NSpin>
    </NCard>

    <!-- 本月收支卡（issue #144）：紧随净资产之后，先看本月现金流再看投资/预算。
         栅格按窗口分级分档（issue #847）：移动档单列（三格纵排，窄屏不横向挤压），
         桌面档保持三列；断点口径接窗口分级 composable，naive 自带断点不用
         （responsive="screen" 在纯数字 cols 下本就惰性，已随本次移除）。 -->
    <NCard :title="t('dashboard.monthly.title')" size="small">
      <NGrid :cols="isMobileTier ? 1 : 3" :x-gap="16">
        <NGridItem>
          <NSpace vertical :size="4">
            <NText depth="3" style="font-size: 12px">{{ t("dashboard.monthly.income") }}</NText>
            <NText strong style="font-size: 20px">{{ formatAmount(netIncomeCents) }}</NText>
          </NSpace>
        </NGridItem>
        <NGridItem>
          <NSpace vertical :size="4">
            <NText depth="3" style="font-size: 12px">{{ t("dashboard.monthly.netExpense") }}</NText>
            <NText strong style="font-size: 20px">{{ formatAmount(netExpenseCents) }}</NText>
          </NSpace>
        </NGridItem>
        <NGridItem>
          <NSpace vertical :size="4">
            <NText depth="3" style="font-size: 12px">{{ t("dashboard.monthly.net") }}</NText>
            <NText strong style="font-size: 20px">{{ formatAmount(balanceCents) }}</NText>
          </NSpace>
        </NGridItem>
      </NGrid>
    </NCard>

    <!-- 投资概览卡（issue #145）：始终展示，无持仓时空态占位 -->
    <NCard
      :title="t('dashboard.investment.title')"
      size="small"
      data-testid="investment-overview-card"
    >
      <!-- 投资概览卡栅格按窗口分级分档（issue #847）：移动档单列，桌面档保持既有
           三列；累计收益卡（issue #1077）与前两格同排。列数用纯数字——NGrid 默认
           responsive="self" 只认数字前缀，具名断点（s:）永不命中会静默退成 1 列
           （三格竖排），断点口径统一接窗口分级常量、不自立断点。 -->
      <!-- 三卡形态与口径归 PortfolioStatsCards（与持仓页签合计区同一组件，issue #902/#1077） -->
      <PortfolioStatsCards
        v-if="holdingRows.length > 0"
        test-id-prefix="dashboard-total-"
        scope="wholeLedger"
        :market-value="statCards.marketValue"
        :unrealized-pnl="statCards.unrealizedPnl"
        :cumulative-pnl="cumulativePnl"
        :native-currency="nativeCurrency"
        :retry="refreshPortfolio"
      />
      <NEmpty v-else :description="t('dashboard.investment.empty')" />
    </NCard>

    <!-- 财务自由度卡（issue #344）：投资概览之后、物品使用成本之前——
         先看资产再看「资产够不够躺」，最后才看单品与预算执行 -->
    <NCard :title="t('dashboard.freedom.title')" size="small" data-testid="financial-freedom-card">
      <template #header-extra>
        <!-- 计算口径说明（3% 乘数使百分比无法从已展示的分子/分母直接推出，
             不解释会被当成算错，ADR-0048）。按输入轴分面（ADR-0088 决策 6 悬停
             一击可达，issue #847）：指针轴悬停即现、移开即关、不拦交互，不在
             ADR-0035 弹层注册表枚举内，用裸 NTooltip（行为零变化）；触控轴悬停
             不可达，改点按气泡（经 AppPopover 入弹层注册表，AmountCell 同款），
             触发器以全局工具类扩热区至 ≥48px。口径文案两轴同源（同一组行）。 -->
        <NTooltip v-if="!isTouch" placement="top" :style="{ maxWidth: '320px' }">
          <template #trigger>
            <NButton
              text
              :aria-label="t('dashboard.freedom.tooltipAria')"
              data-testid="financial-freedom-info"
            >
              <NIcon :size="14" color="var(--n-title-text-color, #999)">
                <InformationCircleOutline />
              </NIcon>
            </NButton>
          </template>
          <NSpace vertical :size="2">
            <span v-for="key in FREEDOM_INFO_KEYS" :key="key">{{
              t(`dashboard.freedom.${key}`)
            }}</span>
          </NSpace>
        </NTooltip>
        <AppPopover v-else trigger="click" placement="top" :style="{ maxWidth: '320px' }">
          <template #trigger>
            <!-- 热区外扩量覆写：text 中号按钮高 34px，默认 ±6px 垂直仅 46px 不达
                 48px 基线，按头部无跨行误触场景加大至 ±8px（ReportsView 同型）。 -->
            <NButton
              text
              class="touch-hit-area"
              :style="{ '--touch-hit-inset': '-8px -14px' }"
              :aria-label="t('dashboard.freedom.tooltipAria')"
              data-testid="financial-freedom-info"
            >
              <NIcon :size="14" color="var(--n-title-text-color, #999)">
                <InformationCircleOutline />
              </NIcon>
            </NButton>
          </template>
          <NSpace vertical :size="2">
            <span v-for="key in FREEDOM_INFO_KEYS" :key="key">{{
              t(`dashboard.freedom.${key}`)
            }}</span>
          </NSpace>
        </AppPopover>
      </template>
      <NSpin :show="freedomLoading">
        <!-- 零分母（未设预算）：占位引导跳预算页，口径不回退实际支出；
             按钮放 #extra——NEmpty 的 default slot 会顶掉 description -->
        <NEmpty
          v-if="freedom && freedom.denominator_cents === 0"
          :description="t('dashboard.freedom.zeroDenominator')"
        >
          <template #extra>
            <NButton size="small" type="primary" @click="goBudget">
              {{ t("dashboard.freedom.goSetupBudget") }}
            </NButton>
          </template>
        </NEmpty>
        <!-- 缺汇率等报错：卡内警告（沿物品使用成本卡的警告先例）+ 重试
             （重试为本卡按验收新增，先例仅警告无重试；重试即 refresh） -->
        <NAlert v-else-if="freedomError" type="warning" :bordered="false">
          <NSpace align="center" :size="8">
            <span>{{ freedomError }}</span>
            <NButton size="tiny" quaternary type="warning" @click="refreshFreedom">
              {{ t("dashboard.freedom.retry") }}
            </NButton>
          </NSpace>
        </NAlert>
        <NSpace v-else-if="freedom && freedomStage" vertical :size="4">
          <NSpace align="center" justify="space-between" style="width: 100%">
            <NText strong style="font-size: 28px" data-testid="financial-freedom-ratio">
              {{ freedom.ratio }}%
            </NText>
            <NTag :type="freedomStage.type" size="small" data-testid="financial-freedom-stage">
              {{ freedomStage.label }}
            </NTag>
          </NSpace>
          <NProgress
            type="line"
            :percentage="freedomProgress.percentage"
            :status="freedomProgress.success ? 'success' : undefined"
            :show-indicator="false"
          />
          <NText depth="3" style="font-size: 12px">
            {{
              t("dashboard.freedom.numeratorLabel", {
                amount: formatAmount(freedom.numerator_cents, freedomCurrency),
              })
            }}
            ·
            {{
              t("dashboard.freedom.denominatorLabel", {
                amount: formatAmount(freedom.denominator_cents, freedomCurrency),
              })
            }}
          </NText>
          <NText depth="3" style="font-size: 12px">
            {{ t("dashboard.freedom.coverageYears", { years: freedom.coverage_years }) }}
          </NText>
        </NSpace>
      </NSpin>
    </NCard>
  </NSpace>
</template>
