<script setup lang="ts">
import { NAlert, NButton, NGi, NGrid, NStatistic, NText } from "naive-ui";
import { computed } from "vue";
import { t } from "@ledger/i18n";
import { formatAmount } from "@ledger/money";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import { useWindowTier } from "@ledger/window-tier";
import { pnlSemanticColor } from "@ledger/theme/semantic-colors";
import ConceptLabel from "@/investment/ConceptLabel.vue";
import type { ConceptKey, ConceptScope } from "@/investment/concept-tips";
import type { StatCardValue } from "@/investment/usePortfolioOverview";
import { statsCard, statsLabel, statsValue } from "./portfolio-stats.css.ts";

const props = defineProps<{
  marketValue: StatCardValue;
  unrealizedPnl: StatCardValue;
  cumulativePnl: StatCardValue;
  nativeCurrency: string | null;
  testIdPrefix: string;
  scope: ConceptScope;
  retry?: () => void;
}>();

const reference = useReferenceStore();
const appStore = useAppStore();
const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

interface StatCard {
  testId: string;
  label: string;
  concept: ConceptKey;
  scope: ConceptScope;
  total: StatCardValue;
  text: string | null;
  color: string | undefined;
  warning: string | null;
}

const stats = computed<StatCard[]>(() => {
  const currency = reference.getCurrency(props.nativeCurrency ?? "");
  const build = (
    total: StatCardValue,
    concept: StatCard["concept"],
    pnl: boolean,
  ): Pick<StatCard, "total" | "concept" | "text" | "color" | "warning"> => ({
    total,
    concept,
    text: total.cents === null ? null : formatAmount(total.cents, currency),
    color: !pnl || total.cents === null ? undefined : pnlSemanticColor(total.cents, appStore.theme),
    warning:
      total.error ?? (total.rateMissingCount > 0 ? t("investments.statsCards.rateMissing") : null),
  });
  return [
    {
      testId: `${props.testIdPrefix}market-value`,
      label: t("investments.concepts.marketValue"),
      scope: props.scope,
      ...build(props.marketValue, "marketValue", false),
    },
    {
      testId: `${props.testIdPrefix}unrealized-pnl`,
      label: t("investments.concepts.unrealizedPnl"),
      scope: props.scope,
      ...build(props.unrealizedPnl, "unrealizedPnl", true),
    },
    {
      testId: `${props.testIdPrefix}cumulative-pnl`,
      label: t("investments.concepts.cumulativePnl"),
      scope: "wholeLedger",
      ...build(props.cumulativePnl, "cumulativePnl", true),
    },
  ];
});

function onRetry() {
  props.retry?.();
}
</script>

<template>
  <NGrid :x-gap="16" :y-gap="12" :cols="isMobileTier ? 1 : 3">
    <NGi v-for="stat in stats" :key="stat.testId">
      <NStatistic :class="statsCard" :data-testid="stat.testId" tabular-nums>
        <template #label>
          <span :class="statsLabel">
            <!-- 概念名自成一个元素：与触发器之间的纯空白节点被编译器去除，
                 标签文案不因加图标而多出空格（卡片 .text() 逐字口径不变） -->
            <ConceptLabel
              :label="stat.label"
              :concept="stat.concept"
              :scope="stat.scope"
              :test-id="stat.testId"
            />
          </span>
        </template>
        <!-- 缺料显式可见（issue #1797 / ADR-0131 修订）：报错或缺折算汇率 →
             整卡警告 + 重试（重试即重发同一条读命令），不给半截数字 -->
        <NAlert
          v-if="stat.warning"
          type="warning"
          :bordered="false"
          :data-testid="`${stat.testId}-warning`"
        >
          <NSpace align="center" :size="8">
            <span>{{ stat.warning }}</span>
            <NButton
              v-if="retry"
              size="tiny"
              quaternary
              type="warning"
              :data-testid="`${stat.testId}-retry`"
              @click="onRetry"
            >
              {{ t("investments.statsCards.retry") }}
            </NButton>
          </NSpace>
        </NAlert>
        <span v-else :class="statsValue" :data-testid="`${stat.testId}-value`">
          <template v-if="stat.text === null">-</template>
          <template v-else>
            <span :style="stat.color ? { color: stat.color } : undefined">{{ stat.text }}</span>
          </template>
        </span>
        <!-- 缺现价未计入计数（>0 小字标注）：只随数值态出现，警告态不重复交代 -->
        <NText
          v-if="!stat.warning && stat.total.missingPriceCount > 0"
          depth="3"
          style="font-size: 12px"
          :data-testid="`${stat.testId}-missing-price`"
        >
          {{ t("investments.statsCards.missingPrice", { count: stat.total.missingPriceCount }) }}
        </NText>
      </NStatistic>
    </NGi>
  </NGrid>
</template>
