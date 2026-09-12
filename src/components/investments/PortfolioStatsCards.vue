<script setup lang="ts">
import { NButton, NGi, NGrid, NIcon, NStatistic, NTooltip } from 'naive-ui'
import { InformationCircleOutline } from '@vicons/ionicons5'
import { computed } from 'vue'
import { t } from '@ledger/i18n'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { useInputMode } from '@/composables/useInputMode'
import { useWindowTier } from '@/composables/useWindowTier'
import { pnlSemanticColor } from '@/theme/semantic-colors'
import AppPopover from '@/components/AppPopover.vue'
import { currencyAmountSegments, type CurrencyAmountGroup } from '@/composables/usePortfolioOverview'
import { statsCard, statsLabel, statsSeparator, statsValue } from './portfolio-stats.css.ts'

/**
 * 投资合计三卡（总市值 / 持仓收益 / 累计收益，issue #902 / #1077）：持仓页签
 * 合计区与首页投资概览卡共用**同一份**形态与展示口径，避免两处漂移——
 * - 三卡同排：桌面档三列、移动档单列，列数用纯数字（NGrid 默认
 *   responsive="self" 只认数字前缀，具名断点 `s:` 永不命中会静默退成 1 列），
 *   断点口径接窗口分级唯一事实源、不自立断点；
 * - 颜色：盈亏两卡逐币种按自身符号着盈亏涨跌色（红涨绿跌），总市值卡保持中性
 *   （词汇表「盈亏涨跌色」——涨跌色不外溢到市值）；
 * - 概念说明按输入轴分面（ADR-0088 决策 6 悬停一击可达，先例见首页财务自由度卡）：
 *   指针轴悬停即现（裸 NTooltip，不在 ADR-0035 弹层注册表枚举内）；触控轴悬停
 *   不可达，改点按气泡（经 AppPopover 入弹层注册表），触发器以全局工具类扩热区；
 *   文案两轴同源，且标签/口径/aria 三个概念各一份（i18n `investments.concepts`）。
 *
 * 纯展示：三组按币种合计与卡片 testid 前缀由消费方传入，取数口径留在各页。
 */
const props = defineProps<{
  /** 按币种分组的总市值合计 */
  marketValueGroups: CurrencyAmountGroup[]
  /** 按币种分组的持仓收益（未实现盈亏）合计 */
  unrealizedPnlGroups: CurrencyAmountGroup[]
  /** 按币种分组的累计收益合计（全账本口径） */
  cumulativePnlGroups: CurrencyAmountGroup[]
  /** 卡片 data-testid 前缀（含结尾连字符）：持仓页 `total-`、首页 `dashboard-total-` */
  testIdPrefix: string
}>()

const reference = useReferenceStore()
const appStore = useAppStore()
const windowTier = useWindowTier()
const inputMode = useInputMode()
const isMobileTier = computed(() => windowTier.value === 'mobile')
const isTouch = computed(() => inputMode.value === 'touch')

// 三卡一次算好：标签/口径说明取自 investments.concepts（投资域概念的唯一文案源），
// 分组段走 currencyAmountSegments（与 formatCurrencyGroups 同一分组展示单点）。
const stats = computed(() => [
  {
    testId: `${props.testIdPrefix}market-value`,
    label: t('investments.concepts.marketValue'),
    tip: t('investments.concepts.marketValueTip'),
    pnl: false,
    segments: currencyAmountSegments(props.marketValueGroups, reference.currencyMap),
  },
  {
    testId: `${props.testIdPrefix}unrealized-pnl`,
    label: t('investments.concepts.unrealizedPnl'),
    tip: t('investments.concepts.unrealizedPnlTip'),
    pnl: true,
    segments: currencyAmountSegments(props.unrealizedPnlGroups, reference.currencyMap),
  },
  {
    testId: `${props.testIdPrefix}cumulative-pnl`,
    label: t('investments.concepts.cumulativePnl'),
    tip: t('investments.concepts.cumulativePnlTip'),
    pnl: true,
    segments: currencyAmountSegments(props.cumulativePnlGroups, reference.currencyMap),
  },
])

/** 分组段的内联色：盈亏卡走盈亏涨跌色（红涨绿跌、随主题换变体），市值卡返回空 */
function statValueStyle(pnl: boolean, cents: number) {
  return pnl ? { color: pnlSemanticColor(cents, appStore.theme) } : undefined
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
            <span>{{ stat.label }}</span>
            <NTooltip v-if="!isTouch" placement="top" :style="{ maxWidth: '320px' }">
              <template #trigger>
                <NButton
                  text
                  :aria-label="t('investments.concepts.tipAria', { concept: stat.label })"
                  :data-testid="`${stat.testId}-info`"
                >
                  <NIcon :size="14" color="var(--n-label-text-color, #999)">
                    <InformationCircleOutline />
                  </NIcon>
                </NButton>
              </template>
              {{ stat.tip }}
            </NTooltip>
            <AppPopover v-else trigger="click" placement="top" :style="{ maxWidth: '320px' }">
              <template #trigger>
                <NButton
                  text
                  class="touch-hit-area"
                  :style="{ '--touch-hit-inset': '-10px -10px' }"
                  :aria-label="t('investments.concepts.tipAria', { concept: stat.label })"
                  :data-testid="`${stat.testId}-info`"
                >
                  <NIcon :size="14" color="var(--n-label-text-color, #999)">
                    <InformationCircleOutline />
                  </NIcon>
                </NButton>
              </template>
              {{ stat.tip }}
            </AppPopover>
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
