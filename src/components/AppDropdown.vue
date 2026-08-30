<script setup lang="ts">
import { useAttrs, watch } from 'vue'
import { NDropdown } from 'naive-ui'
import { createOverlayToken } from '@/composables/overlayRegistry'

// 薄封装 NDropdown，接入弹层注册表（ADR-0035）：应用内的 NDropdown 一律经本
// 组件使用，菜单开/关状态实时上报，驱动快捷键抑制。其余 props/attrs/slots
// 原样透传。
//
// 刻意不声明 show prop（原因见 AppSelect 注释）：:show / @update:show 经 attrs
// 原样透传；非受控开合与 clickoutside/选中触发的关闭经根上的 update:show 监听
// 上报，trigger="manual" 下调用方直接改 :show prop 的开合由 attrs watch 兜底。
const attrs = useAttrs()
const overlay = createOverlayToken('dropdown')
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
  <NDropdown @update:show="onUpdateShow">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NDropdown>
</template>
