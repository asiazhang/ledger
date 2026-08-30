<script setup lang="ts">
import { useAttrs, watch } from 'vue'
import { NTreeSelect } from 'naive-ui'
import { createOverlayToken } from '@/composables/overlayRegistry'

// 薄封装 NTreeSelect，接入弹层注册表（ADR-0035）：应用内的 NTreeSelect 一律经
// 本组件使用，菜单开/关状态实时上报，驱动快捷键抑制。其余 props/attrs/slots
// 原样透传。
//
// 刻意不声明 show prop（原因见 AppSelect 注释）：:show / @update:show 经 attrs
// 原样透传；开合经根上的 update:show 监听上报，受控调用方直接改 :show prop
// 的开合由 attrs watch 兜底。
const attrs = useAttrs()
const overlay = createOverlayToken('tree-select')
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
  <NTreeSelect @update:show="onUpdateShow">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NTreeSelect>
</template>
