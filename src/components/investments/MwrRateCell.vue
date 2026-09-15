<script setup lang="ts">
import { computed } from 'vue'
import { NTooltip } from 'naive-ui'
import { formatRate } from '@ledger/money'
import { t } from '@ledger/i18n'
import AppPopover from '@ledger/ui-kit/AppPopover.vue'
import { useInputMode } from '@/composables/useInputMode'
import { marker, trigger } from './mwr-rate-cell.css.ts'

/**
 * 收益率单元格（ADR-0088 决策 6 悬停一击可达 / issue #1343 口径标注）：只承载
 * 可计算值的展示与输入轴交互替换，不持业务语义——
 * - 年化（缺省）：纯文本 span，桌面与触控零变化；
 * - 未年化（`annualized = false`）：百分数后跟角标「*」——ADR-0115 修订防误读
 *   诉求的在场标注，但不占列宽；解释文案两轴同源（i18n
 *   `investments.pnl.cumulativeTip`），指针轴悬停即现（裸 NTooltip，与合计三卡
 *   概念说明同款）、触控轴点按弹出（经 AppPopover 入弹层注册表）。
 *
 * 颜色由调用方计算传入（pnlSemanticColor 口径不变，归 renderMwrRateCell 三态
 * 单点）；输入轴信号经 useInputMode 唯一事实源消费，换轴实时切换形态
 * （AmountCell 同款分工：三态分流留在调用方，组件只管输入轴形态）。
 */
const props = defineProps<{
  /** 可计算利率（小数，formatRate 展示） */
  rate: number
  /** 盈亏涨跌色（调用方按主题取的 pnlSemanticColor 产物） */
  color: string
  /** 是否年化口径（false = 未年化，带角标与解释） */
  annualized?: boolean
}>()

const inputMode = useInputMode()
const isTouch = computed(() => inputMode.value === 'touch')

const text = computed(() => formatRate(props.rate))
const tip = computed(() => t('investments.pnl.cumulativeTip'))
const marked = computed(() => props.annualized === false)

/** 触控触发器的读屏替代：数值 + 口径解释一并可达（指针轴不挂 role，零变化） */
const ariaLabel = computed(() => (marked.value ? `${text.value}，${tip.value}` : text.value))
</script>

<template>
  <NTooltip v-if="marked && !isTouch" placement="top" :style="{ maxWidth: '320px' }">
    <template #trigger>
      <span data-mwr-marker :class="trigger" :style="{ color }">{{ text }}<sup :class="marker">*</sup></span>
    </template>
    {{ tip }}
  </NTooltip>
  <AppPopover v-else-if="marked" trigger="click" placement="top" :style="{ maxWidth: '320px' }">
    <template #trigger>
      <span
        data-mwr-marker
        class="touch-hit-area"
        role="button"
        :aria-label="ariaLabel"
        :class="trigger"
        :style="{ color }"
      >
        {{ text }}<sup :class="marker">*</sup>
      </span>
    </template>
    {{ tip }}
  </AppPopover>
  <span v-else :style="{ color }">{{ text }}</span>
</template>
