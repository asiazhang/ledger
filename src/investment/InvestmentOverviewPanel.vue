<script setup lang="ts">
import { NAlert, NButton, NCard, NSpace, NSpin, NText } from "naive-ui";
import { computed } from "vue";
import { t } from "@ledger/i18n";
import { formatAmount, amountPrivacyEnabled } from "@ledger/money";
import { pnlSemanticColor } from "@ledger/theme/semantic-colors";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import ConceptLabel from "@/investment/ConceptLabel.vue";
import { useInvestmentOverview } from "@/investment/useInvestmentOverview";
import PortfolioTrendPanel from "@/investment/PortfolioTrendPanel.vue";
import {
  barCash,
  barHoldings,
  compositionBar,
  hero,
  heroHeader,
  heroValue,
  legend,
  legendDotCash,
  legendDotHoldings,
  legendItem,
} from "./investment-overview.css.ts";

const reference = useReferenceStore();
const appStore = useAppStore();
const { data, loading, error, refresh } = useInvestmentOverview();

const currency = computed(() => reference.getCurrency(data.value?.native_currency ?? ""));

function amount(cents: number): string {
  return formatAmount(cents, currency.value);
}

const totalItems = computed(() => [
  {
    testId: "overview-total-market-value",
    label: t("investments.concepts.marketValue"),
    concept: "marketValue" as const,
    pnl: false,
    cents: data.value?.total_market_value_cents ?? 0,
  },
  {
    testId: "overview-unrealized-pnl",
    label: t("investments.concepts.unrealizedPnl"),
    concept: "unrealizedPnl" as const,
    pnl: true,
    cents: data.value?.unrealized_pnl_cents ?? 0,
  },
  {
    testId: "overview-cumulative-pnl",
    label: t("investments.concepts.cumulativePnl"),
    concept: "cumulativePnl" as const,
    pnl: true,
    cents: data.value?.cumulative_pnl_cents ?? 0,
  },
]);

function totalValueStyle(pnl: boolean, cents: number): { color: string } | undefined {
  return pnl ? { color: pnlSemanticColor(cents, appStore.theme) } : undefined;
}

function sharePct(legCents: number, totalCents: number): number {
  if (totalCents <= 0) return 0;
  return Math.min(100, Math.max(0, (legCents / totalCents) * 100));
}

const cashSharePct = computed(() =>
  sharePct(data.value?.investment_cash_cents ?? 0, data.value?.investable_assets_cents ?? 0),
);
const holdingsSharePct = computed(() =>
  sharePct(data.value?.holdings_market_value_cents ?? 0, data.value?.investable_assets_cents ?? 0),
);

const legs = computed(() => [
  {
    testId: "overview-cash-leg",
    label: t("investments.overview.cashLeg"),
    cents: data.value?.investment_cash_cents ?? 0,
    dotClass: legendDotCash,
  },
  {
    testId: "overview-holdings-leg",
    label: t("investments.overview.holdingsLeg"),
    cents: data.value?.holdings_market_value_cents ?? 0,
    dotClass: legendDotHoldings,
  },
]);

const showComposition = computed(
  () => !amountPrivacyEnabled.value && (data.value?.investable_assets_cents ?? 0) > 0,
);

function retry(): void {
  void refresh();
}
</script>

<template>
  <NCard :title="t('investments.overview.title')" data-testid="investment-overview">
    <NSpin :show="loading">
      <!-- 缺折算汇率等报错：整卡警告 + 重试，不显示半截数字（后端码化错误上抛，
           与财务自由度卡同款形态） -->
      <NAlert v-if="error" type="warning" data-testid="overview-error">
        <NSpace align="center" :size="8">
          <span>{{ error }}</span>
          <NButton
            size="tiny"
            quaternary
            type="warning"
            data-testid="overview-retry"
            @click="retry"
          >
            {{ t("investments.overview.retry") }}
          </NButton>
        </NSpace>
      </NAlert>

      <NSpace v-else-if="data" vertical :size="12">
        <!-- 首屏焦点（spec #1684）：可投资资产大号数字独占首行，本位币标注退到
             右上角——打开页面第一眼即回答「我一共有多少可投资资产」；
             中性主文本色，不着涨跌色（涨跌色不外溢）。 -->
        <div :class="hero">
          <div :class="heroHeader">
            <NText depth="3">
              <ConceptLabel
                :label="t('investments.overview.investableAssets')"
                concept="investableAssets"
                test-id="overview-investable-assets"
              />
            </NText>
            <NText depth="3" data-testid="overview-currency">
              {{ t("investments.overview.currencyLabel", { currency: data.native_currency }) }}
            </NText>
          </div>
          <div :class="[heroValue, 'tabular-nums']" data-testid="overview-investable-assets-value">
            {{ amount(data.investable_assets_cents) }}
          </div>
        </div>

        <!-- 构成可视化（spec #1684）：两段式比例条（段宽 = 各腿 ÷ 可投资资产，
             展示派生纯图形、不显示百分数） + 图例（色点 + 既有两腿标签键 +
             格式化金额）。比例条 aria-hidden，等价信息由图例文本承担。 -->
        <div
          v-if="showComposition"
          :class="compositionBar"
          data-testid="overview-composition-bar"
          aria-hidden="true"
        >
          <div
            :class="barCash"
            data-testid="overview-bar-cash"
            :style="{ width: cashSharePct + '%' }"
          />
          <div
            :class="barHoldings"
            data-testid="overview-bar-holdings"
            :style="{ width: holdingsSharePct + '%' }"
          />
        </div>

        <div :class="legend">
          <span v-for="leg in legs" :key="leg.testId" :class="legendItem">
            <span :class="leg.dotClass" aria-hidden="true" />
            <span :data-testid="leg.testId">{{ leg.label }} {{ amount(leg.cents) }}</span>
          </span>
        </div>

        <!-- 投资合计三项（#1537）：与可投资资产同页并读；标签沿用既有概念键，
             概览页的口径差异由 scope 变体句承担（同一标签在持仓页签是分组口径） -->
        <NSpace vertical :size="8">
          <NText depth="3" data-testid="overview-totals-title">{{
            t("investments.overview.totalsTitle")
          }}</NText>
          <NSpace :size="24" :wrap="true">
            <NSpace
              v-for="item in totalItems"
              :key="item.testId"
              vertical
              :size="2"
              :data-testid="item.testId"
            >
              <NText depth="3">
                <ConceptLabel
                  :label="item.label"
                  :concept="item.concept"
                  scope="overview"
                  :test-id="item.testId"
                />
              </NText>
              <NText
                :data-testid="`${item.testId}-value`"
                :style="totalValueStyle(item.pnl, item.cents)"
                >{{ amount(item.cents) }}</NText
              >
            </NSpace>
          </NSpace>
        </NSpace>

        <!-- 缺价持仓跳过不计入，但显式说明数量：既不虚增也不静默低估 -->
        <NText
          v-if="data.missing_price_holding_count > 0"
          depth="3"
          data-testid="overview-missing-price"
        >
          {{ t("investments.overview.missingPrice", { count: data.missing_price_holding_count }) }}
        </NText>

        <!-- 没有投资账户：数字照常 0 + 一句引导（引导只陈述下一步，不设动作入口） -->
        <NText v-if="!data.has_investment_account" depth="3" data-testid="overview-no-account">
          {{ t("investments.overview.noAccount") }}
        </NText>
      </NSpace>
    </NSpin>
  </NCard>

  <!-- 组合走势卡（#1907）：自退役的走势页签迁入；区间切换与空态三态随卡走 -->
  <NCard :title="t('investments.trend.cardTitle')" data-testid="overview-trend-card">
    <PortfolioTrendPanel />
  </NCard>
</template>
