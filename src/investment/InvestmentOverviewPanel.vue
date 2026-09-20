<script setup lang="ts">
import { NAlert, NButton, NCard, NSpace, NSpin, NText } from "naive-ui";
import { computed } from "vue";
import { t } from "@ledger/i18n";
import { formatAmount } from "@ledger/money";
import { useReferenceStore } from "@/stores/reference";
import ConceptLabel from "@/investment/ConceptLabel.vue";
import { useInvestmentOverview } from "@/investment/useInvestmentOverview";

/**
 * 投资概览面板（spec #1532 / issue #1536）：投资页「概览」页签的内容本体——
 * 可投资资产一个本位币数字，加「投资账户现金 / 持仓市值」两腿拆分。
 *
 * 纯只读：页内没有同步、录价、设置预算等写入口或动作入口，唯一的交互是
 * 缺折算汇率时的卡内「重试」（重试 = 重发同一条读命令，不产生任何写入）。
 * 缺料状态显式可见：缺现价持仓按既有空值语义跳过但给出未计入数量说明；
 * 没有投资账户时数字照常显示 0 并给一句引导——不隐藏功能、不以零虚增。
 *
 * 口径与折算全在后端单点（`investment_overview`，ADR-0130：全页折本位币单值；
 * 与持仓视图「按账户币种分组、不跨币种合并」分工）：本组件只做装配与格式化。
 */
const reference = useReferenceStore();
const { data, loading, error, refresh } = useInvestmentOverview();

/** 金额格式化币种 = 后端透出的本位币（参考数据未就绪时降级为无符号两位小数） */
const currency = computed(() => reference.getCurrency(data.value?.native_currency ?? ""));

function amount(cents: number): string {
  return formatAmount(cents, currency.value);
}

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
        <NSpace vertical :size="2">
          <NText depth="3">
            <ConceptLabel
              :label="t('investments.overview.investableAssets')"
              concept="investableAssets"
              test-id="overview-investable-assets"
            />
          </NText>
          <NText strong style="font-size: 28px" data-testid="overview-investable-assets-value">
            {{ amount(data.investable_assets_cents) }}
          </NText>
          <NText depth="3" data-testid="overview-currency">
            {{ t("investments.overview.currencyLabel", { currency: data.native_currency }) }}
          </NText>
        </NSpace>

        <!-- 两腿拆分：合计的两个来源各一行，窄屏换行排布 -->
        <NSpace :size="16" :wrap="true">
          <NText data-testid="overview-cash-leg">
            {{ t("investments.overview.cashLeg") }} {{ amount(data.investment_cash_cents) }}
          </NText>
          <NText data-testid="overview-holdings-leg">
            {{ t("investments.overview.holdingsLeg") }}
            {{ amount(data.holdings_market_value_cents) }}
          </NText>
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
