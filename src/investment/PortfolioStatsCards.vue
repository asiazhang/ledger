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

/**
 * 投资合计三卡（总市值 / 持仓收益 / 累计收益，issue #902 / #1077）：持仓页签
 * 合计区与首页投资概览卡共用**同一份**形态与展示口径，避免两处漂移——
 * - 三卡同排：桌面档三列、移动档单列，列数用纯数字（NGrid 默认
 *   responsive="self" 只认数字前缀，具名断点 `s:` 永不命中会静默退成 1 列），
 *   断点口径接窗口分级唯一事实源、不自立断点；
 * - 单值（issue #1797，ADR-0131 修订）：三卡改折全局默认币种（本位币）单值，
 *   多币种分组拼接退役——折算与求和全在后端/行集装配完成，本组件只格式化；
 * - 缺料显式可见：缺现价未计入的持仓数小字标注（既不虚增也不静默低估）；
 *   有金额但缺折算汇率（行集软折算失败）或命令码化上抛（如累计收益缺汇率）
 *   → 整卡警告 + 重试，不给半截数字；
 * - 颜色：盈亏两卡按单值符号着盈亏涨跌色（红涨绿跌），总市值卡保持中性
 *   （词汇表「盈亏涨跌色」——涨跌色不外溢到市值）；
 * - 概念说明归 ConceptLabel（issue #1369：标签 + 常驻 ⓘ、按输入轴分面，ADR-0088
 *   决策 6），本组件只声明每个卡的 concept 键与作用域，文案与双轴形态不再在本文件。
 *
 * 纯展示：三卡单值与卡片 testid 前缀由消费方传入，取数口径留在各页。
 */
const props = defineProps<{
  /** 折本位币单值的总市值合计 */
  marketValue: StatCardValue;
  /** 折本位币单值的持仓收益（未实现盈亏）合计 */
  unrealizedPnl: StatCardValue;
  /** 折本位币单值的累计收益合计（全账本口径） */
  cumulativePnl: StatCardValue;
  /** 折算基准币种代码（三卡同币，全局默认币种）：参考数据未就绪时降级裸数字 */
  nativeCurrency: string | null;
  /** 卡片 data-testid 前缀（含结尾连字符）：持仓页 `total-`、首页 `dashboard-total-` */
  testIdPrefix: string;
  /**
   * 页面作用域：持仓页合计随过滤子集更新（`filtered`）、首页恒为全部持仓
   * （`wholeLedger`）。累计收益不受此影响，逐卡覆写为 `wholeLedger`。
   */
  scope: ConceptScope;
  /** 缺料警告态的重试入口（重发同一条读命令，不产生任何写入）；缺省不渲染重试按钮 */
  retry?: () => void;
}>();

const reference = useReferenceStore();
const appStore = useAppStore();
const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

// 三卡一次算好：标签取自 investments.concepts（投资域概念的唯一文案源）、口径说明
// 由 ConceptLabel 按 concept 键现取（同源，调用方给不出第二份措辞），
// 单值经 formatAmount 一次格式化（折算基准币种由 nativeCurrency 统一给符号）。
interface StatCard {
  testId: string;
  label: string;
  /** 概念闭集成员（concept-tips.ts）：拼错即编译期报错，不进 i18n 缺 key 路径 */
  concept: ConceptKey;
  scope: ConceptScope;
  total: StatCardValue;
  /** 格式化单值文本（无可计入行时 null → 展示「-」） */
  text: string | null;
  /** 单值内联色：盈亏卡走盈亏涨跌色（红涨绿跌、随主题换变体），其余为空 */
  color: string | undefined;
  /** 整卡警告文案（命令报错或缺折算汇率）：置位即警告态，不显示半截数字 */
  warning: string | null;
}

const stats = computed<StatCard[]>(() => {
  const currency = reference.getCurrency(props.nativeCurrency ?? "");
  // 单卡共易变部分（testId/label/scope 归各条目自声明，pnl 只用于取色不入卡态）
  const build = (
    total: StatCardValue,
    concept: StatCard["concept"],
    pnl: boolean,
  ): Pick<StatCard, "total" | "concept" | "text" | "color" | "warning"> => ({
    total,
    concept,
    text: total.cents === null ? null : formatAmount(total.cents, currency),
    color: !pnl || total.cents === null ? undefined : pnlSemanticColor(total.cents, appStore.theme),
    // 命令报错优先（累计收益缺汇率码化上抛）；其次行集缺汇率（软折算失败计数）
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
      // 累计收益两处都是全账本口径：不随持仓页的搜索/账户过滤收窄
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
