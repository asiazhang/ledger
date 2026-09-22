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

/**
 * 投资概览面板（spec #1532 / issue #1536、#1537；焦点 Hero 重排 spec #1684）：
 * 投资页「概览」页签的内容本体——首屏焦点是可投资资产的大号数字（本位币标注
 * 退到右上角），其下依次是两腿构成的比例条 + 图例、投资合计三项一行细条
 * （盈亏两数着盈亏涨跌色，总市值与可投资资产保持中性）。
 *
 * 纯只读：页内没有同步、录价、设置预算等写入口或动作入口，唯一的交互是
 * 缺折算汇率时的卡内「重试」（重试 = 重发同一条读命令，不产生任何写入）与
 * 口径说明。缺料状态显式可见：缺现价持仓按既有空值语义跳过但给出未计入数量
 * 说明；没有投资账户时数字照常显示 0 并给一句引导——不隐藏功能、不以零虚增。
 *
 * 口径与折算全在后端单点（`investment_overview`，ADR-0131：全页折本位币单值；
 * 与持仓视图「按账户币种分组、不跨币种合并」分工）：本组件只做装配与格式化。
 * 合计三项沿用既有标签（概念键同名），币种口径差异由概念说明的概览 scope 变体
 * 句承担（ADR-0131 决策 3 / ADR-0129 先例），不另造标签。
 */
const reference = useReferenceStore();
const appStore = useAppStore();
const { data, loading, error, refresh } = useInvestmentOverview();

/** 金额格式化币种 = 后端透出的本位币（参考数据未就绪时降级为无符号两位小数） */
const currency = computed(() => reference.getCurrency(data.value?.native_currency ?? ""));

function amount(cents: number): string {
  return formatAmount(cents, currency.value);
}

/**
 * 投资合计三项（#1537）：标签取概念命名空间的既有展示词（PortfolioStatsCards
 * 同款，总市值/持仓收益/累计收益），口径说明挂概览 scope 变体；数值全为后端
 * 折本位币单值，前端零算术。
 */
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

/**
 * 合计三项内联色：盈亏两数经盈亏涨跌色语义接缝着色（红涨绿跌、0 归涨色、
 * 随主题取变体，与投资合计三卡同接缝同边界），总市值保持中性（无内联色）。
 */
function totalValueStyle(pnl: boolean, cents: number): { color: string } | undefined {
  return pnl ? { color: pnlSemanticColor(cents, appStore.theme) } : undefined;
}

/**
 * 构成比例条段宽（spec #1684，展示派生）：两段**各自独立**按「各腿 ÷ 可投资
 * 资产」计算（用户故事 27 字面）——不做 `100 − x` 余量法，前端不复述「两腿之和
 * = 可投资资产」的口径不变量（ADR-0131「前端零算术」的边界：金额口径仍零前端
 * 算术，这里只画比例，不产生任何用户可见数字、不进文案）。若后端两腿与合计失配，
 * 比例条如实出现缺口而非被余量抹平。总额 ≤ 0 时不产段宽（避开 0/0）；钳 [0, 100]
 * 防异常输入出负宽/超宽。
 */
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

/**
 * 图例两腿（spec #1684）：色点 + 既有两腿标签键 + 格式化金额——比例条
 * aria-hidden，辅助技术的等价信息由本图例承担；文案零新增键。
 */
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

/**
 * 比例条隐藏边界（spec #1684 显式裁量，已确认）：可投资资产为 0 时空轨道易被
 * 误读为异常或加载残缺；金额隐私模式开启时段宽绕过统一格式化函数的掩码，按
 * 最严口径一并隐藏（相对构成也不泄露）。两处隐藏的都只是图形——金额、图例与
 * 引导语义照常。图例不受影响：它经 formatAmount 走统一掩码。读 amountPrivacyEnabled
 * 建立依赖，开关翻转即重渲染。
 */
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
</template>
