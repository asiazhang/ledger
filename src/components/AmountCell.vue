<script setup lang="ts">
import { computed } from 'vue'
import AppPopover from '@/components/AppPopover.vue'
import { useInputMode } from '@/composables/useInputMode'

/**
 * 金额单元格（ADR-0088 决策 6 悬停一击可达 · 交易表金额全文，issue #843）：
 * 纯展示组件，只承载输入轴交互替换，不持业务语义——
 * - 指针轴：纯文本 span（与既有渲染一致，无悬停行为，桌面零变化）；
 * - 触控轴：金额成为点按触发器，点按弹出全文（「点按查看」，经 AppPopover
 *   入弹层注册表）。金额全文文案与触发器文案同源（同一 formatAmount 产物，
 *   含隐私掩码态），固定列宽下的换行/截断不再是触控端获取全文的唯一途径。
 *
 * 文案与语义色由调用方计算传入（formatAmount / kindSemanticColor 口径不变，
 * 归 transaction-columns 列配置单点）；输入轴信号经 useInputMode 唯一事实源
 * 消费，换轴实时切换形态。
 */
const props = defineProps<{
  /** 展示文案（已按金额口径格式化，含隐私掩码态） */
  text: string
  /** 语义色（kindSemanticColor 取值，随调用方主题响应式重建） */
  color: string
}>()

const inputMode = useInputMode()
const isTouch = computed(() => inputMode.value === 'touch')
const cellStyle = computed(() => ({ color: props.color }))
</script>

<template>
  <AppPopover v-if="isTouch" trigger="click" placement="top">
    <template #trigger>
      <span class="amount-cell touch-hit-area" :style="cellStyle" role="button" :aria-label="text">{{ text }}</span>
    </template>
    <span class="amount-cell-full">{{ text }}</span>
  </AppPopover>
  <span v-else class="amount-cell" :style="cellStyle">{{ text }}</span>
</template>

<style scoped>
/* 刻意不设 white-space / ellipsis：指针轴渲染与既有金额 span 逐字节一致
   （固定列宽下超长换行的既有布局红线，桌面零变化）；触控轴全文经点按
   气泡获得，不靠单元格内截断。 */
.amount-cell-full {
  font-variant-numeric: tabular-nums;
}

/* 触控轴触发器热区外扩（≥48px 抽查）：命中面收敛于全局工具类 touch-hit-area
   （global.css 单点，默认表格单元格外扩量）；指针轴分支不挂该类，桌面命中面
   零变化。 */
</style>
