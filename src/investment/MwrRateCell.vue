<script setup lang="ts">
import { computed } from 'vue'
import { formatRate } from '@ledger/money'
import { t } from '@ledger/i18n'
import ConceptTipHost from '@/investment/ConceptTipHost.vue'
import { marker, trigger } from './mwr-rate-cell.css.ts'

/**
 * 收益率单元格（ADR-0088 决策 6 悬停一击可达 / issue #1343 口径标注）：只承载
 * 可计算值的展示与输入轴交互替换，不持业务语义——
 * - 年化（缺省）：纯文本 span，桌面与触控零变化；
 * - 未年化（`annualized = false`）：百分数后跟角标「*」——ADR-0115 修订防误读
 *   诉求的在场标注，但不占列宽；解释文案两轴同源（i18n
 *   `investments.concepts.mwrCumulativeTip`，issue #1369 起与其余口径说明同住
 *   概念命名空间），指针轴悬停即现（裸 NTooltip，与合计三卡概念说明同款）、
 *   触控轴点按弹出（经 AppPopover 入弹层注册表）。
 *
 * 颜色由调用方计算传入（pnlSemanticColor 口径不变，归 renderMwrRateCell 三态
 * 单点）；输入轴双轴分流归 ConceptTipHost 单点（与 ConceptLabel 同一份实现，
 * 换轴实时切换形态），本组件只持角标形态与读屏替代。
 */
const props = defineProps<{
  /** 可计算利率（小数，formatRate 展示） */
  rate: number
  /** 盈亏涨跌色（调用方按主题取的 pnlSemanticColor 产物） */
  color: string
  /** 是否年化口径（false = 未年化，带角标与解释） */
  annualized?: boolean
}>()

const text = computed(() => formatRate(props.rate))
const tip = computed(() => t('investments.concepts.mwrCumulativeTip'))
const marked = computed(() => props.annualized === false)

/** 触控触发器的读屏替代：数值 + 口径解释一并可达（指针轴不挂 role，零变化） */
const ariaLabel = computed(() => (marked.value ? `${text.value}，${tip.value}` : text.value))
</script>

<template>
  <ConceptTipHost v-if="marked" :text="tip">
    <template #default="{ isTouch }">
      <span
        data-mwr-marker
        :class="isTouch ? [trigger, 'touch-hit-area'] : trigger"
        :role="isTouch ? 'button' : undefined"
        :aria-label="isTouch ? ariaLabel : undefined"
        :style="{ color }"
      >
        {{ text }}<sup :class="marker">*</sup>
      </span>
    </template>
  </ConceptTipHost>
  <span v-else :style="{ color }">{{ text }}</span>
</template>
