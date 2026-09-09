<script setup lang="ts">
import { useAttrs, watch } from 'vue'
import { NPopover } from 'naive-ui'
import { createOverlayToken } from '@/composables/overlayRegistry'

// 薄封装 NPopover，接入弹层注册表（ADR-0035）：应用内的 NPopover 一律经本
// 组件使用，气泡开/关状态实时上报，驱动快捷键抑制。default（内容）/trigger
// 等 slots 与其余 props/attrs 原样透传。
//
// 刻意不声明 show prop（原因见 AppSelect 注释）：:show / @update:show 经 attrs
// 原样透传；非受控开合（trigger="click" 等）经根上的 update:show 监听上报，
// 受控调用方直接改 :show prop 的开合由 attrs watch 兜底。
// 首消费者：交易表金额全文点按查看（ADR-0088 决策 6 悬停一击可达）。
const attrs = useAttrs()
const overlay = createOverlayToken('popover')
const onUpdateShow = (value: boolean) => overlay.set(value)
watch(
  () => attrs.show,
  (value) => {
    if (value !== undefined) overlay.set(Boolean(value))
  },
  { immediate: true },
)
</script>

<template>
  <NPopover @update:show="onUpdateShow">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NPopover>
</template>
