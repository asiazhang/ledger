<script setup lang="ts">
import { useAttrs, watch } from 'vue'
import { NSelect } from 'naive-ui'
import { createOverlayToken } from '@/composables/overlayRegistry'

// 薄封装 NSelect，接入弹层注册表（ADR-0035）：应用内的 NSelect 一律经本组件
// 使用，开/关状态实时上报，驱动快捷键抑制。其余 props/attrs/slots 原样透传。
//
// 刻意不声明 show prop：Vue 对可选 Boolean prop 的缺席值转型为 false（而非
// undefined），一旦声明并把 :show 绑回根组件，未传 show 的非受控用法会被
// 变成「受控关闭」，下拉永远打不开。:show / @update:show 经 attrs 原样透传；
// 非受控开合与受控模式下组件内部触发的开合经根上的 update:show 监听上报，
// 受控调用方直接改 :show prop 的开合由 attrs watch 兜底。
const attrs = useAttrs()
const overlay = createOverlayToken('select')
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
  <NSelect @update:show="onUpdateShow">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NSelect>
</template>
