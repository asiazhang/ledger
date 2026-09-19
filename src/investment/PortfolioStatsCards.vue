<script setup lang="ts">
import { NGi, NGrid, NStatistic } from "naive-ui";
import { computed } from "vue";
import { t } from "@ledger/i18n";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import { useWindowTier } from "@ledger/window-tier";
import { pnlSemanticColor } from "@ledger/theme/semantic-colors";
import ConceptLabel from "@/investment/ConceptLabel.vue";
import type { ConceptKey, ConceptScope } from "@/investment/concept-tips";
import {
  currencyAmountSegments,
  type CurrencyAmountGroup,
  type CurrencyAmountSegment,
} from "@/investment/usePortfolioOverview";
import { statsCard, statsLabel, statsSeparator, statsValue } from "./portfolio-stats.css.ts";

/**
 * 投资合计三卡（总市值 / 持仓收益 / 累计收益，issue #902 / #1077）：持仓页签
 * 合计区与首页投资概览卡共用**同一份**形态与展示口径，避免两处漂移——
 * - 三卡同排：桌面档三列、移动档单列，列数用纯数字（NGrid 默认
 *   responsive="self" 只认数字前缀，具名断点 `s:` 永不命中会静默退成 1 列），
 *   断点口径接窗口分级唯一事实源、不自立断点；
 * - 颜色：盈亏两卡逐币种按自身符号着盈亏涨跌色（红涨绿跌），总市值卡保持中性
 *   （词汇表「盈亏涨跌色」——涨跌色不外溢到市值）；
 * - 概念说明归 ConceptLabel（issue #1369：标签 + 常驻 ⓘ、按输入轴分面，ADR-0088
 *   决策 6），本组件只声明每个卡的 concept 键与作用域，文案与双轴形态不再在本文件。
 *
 * 纯展示：三组按币种合计与卡片 testid 前缀由消费方传入，取数口径留在各页。
 */
const props = defineProps<{
  /** 按币种分组的总市值合计 */
  marketValueGroups: CurrencyAmountGroup[];
  /** 按币种分组的持仓收益（未实现盈亏）合计 */
  unrealizedPnlGroups: CurrencyAmountGroup[];
  /** 按币种分组的累计收益合计（全账本口径） */
  cumulativePnlGroups: CurrencyAmountGroup[];
  /** 卡片 data-testid 前缀（含结尾连字符）：持仓页 `total-`、首页 `dashboard-total-` */
  testIdPrefix: string;
  /**
   * 页面作用域：持仓页合计随过滤子集更新（`filtered`）、首页恒为全部持仓
   * （`wholeLedger`）。累计收益不受此影响，逐卡覆写为 `wholeLedger`。
   */
  scope: ConceptScope;
}>();

const reference = useReferenceStore();
const appStore = useAppStore();
const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

// 三卡一次算好：标签取自 investments.concepts（投资域概念的唯一文案源）、口径说明
// 由 ConceptLabel 按 concept 键现取（同源，调用方给不出第二份措辞），
// 分组段走 currencyAmountSegments（与 formatCurrencyGroups 同一分组展示单点）。
interface StatCard {
  testId: string;
  label: string;
  /** 概念闭集成员（concept-tips.ts）：拼错即编译期报错，不进 i18n 缺 key 路径 */
  concept: ConceptKey;
  scope: ConceptScope;
  pnl: boolean;
  segments: CurrencyAmountSegment[];
}

const stats = computed<StatCard[]>(() => [
  {
    testId: `${props.testIdPrefix}market-value`,
    label: t("investments.concepts.marketValue"),
    concept: "marketValue",
    scope: props.scope,
    pnl: false,
    segments: currencyAmountSegments(props.marketValueGroups, reference.currencyMap),
  },
  {
    testId: `${props.testIdPrefix}unrealized-pnl`,
    label: t("investments.concepts.unrealizedPnl"),
    concept: "unrealizedPnl",
    scope: props.scope,
    pnl: true,
    segments: currencyAmountSegments(props.unrealizedPnlGroups, reference.currencyMap),
  },
  {
    testId: `${props.testIdPrefix}cumulative-pnl`,
    label: t("investments.concepts.cumulativePnl"),
    concept: "cumulativePnl",
    // 累计收益两处都是全账本口径：不随持仓页的搜索/账户过滤收窄
    scope: "wholeLedger",
    pnl: true,
    segments: currencyAmountSegments(props.cumulativePnlGroups, reference.currencyMap),
  },
]);

/** 分组段的内联色：盈亏卡走盈亏涨跌色（红涨绿跌、随主题换变体），市值卡返回空 */
function statValueStyle(pnl: boolean, cents: number) {
  return pnl ? { color: pnlSemanticColor(cents, appStore.theme) } : undefined;
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
        <span :class="statsValue" :data-testid="`${stat.testId}-value`">
          <template v-if="stat.segments.length === 0">-</template>
          <template v-else>
            <template v-for="(segment, index) in stat.segments" :key="segment.currencyCode">
              <span v-if="index > 0" :class="statsSeparator"> / </span>
              <span :style="statValueStyle(stat.pnl, segment.cents)">{{ segment.text }}</span>
            </template>
          </template>
        </span>
      </NStatistic>
    </NGi>
  </NGrid>
</template>
