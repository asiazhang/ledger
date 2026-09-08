<script setup lang="ts">
import { onScopeDispose, useAttrs, watch } from 'vue'
import { NDrawer } from 'naive-ui'
import { createOverlayToken } from '@/composables/overlayRegistry'

// AppDrawer（issue #842 / ADR-0088 决策 4）：薄封装 NDrawer——导航抽屉（移动档壳层，
// 词汇表「导航抽屉」）的唯一使用形态，接入弹层注册表（ADR-0035）：开/关状态实时上报，
// 与其他弹层封装共同驱动快捷键抑制的同一判定闸门。其余 props/attrs 原样透传
// （透传依赖单根节点 attrs fallthrough）。
//
// 刻意不声明 show prop（先例 AppModal / AppDropdown）：受控调用方经 v-model:show
// 的开合由根上的 update:show 监听上报，直接改 :show prop 的开合由 attrs watch 兜底。
//
// 卸载兜底：跨断点换档（窗口分级，ADR-0088 决策 2）时移动壳整体卸载，抽屉可能仍开着
// ——NDrawer 是受控组件，卸载不产生 update:show(false)，注册表会滞留开放态导致
// 快捷键永久抑制；作用域销毁时显式撤销上报（幂等，关闭态重复撤销无副作用）。
const attrs = useAttrs()
const overlay = createOverlayToken('drawer')
const onUpdateShow = (value: boolean) => overlay.set(value)
watch(
  () => attrs.show,
  (value) => {
    if (value !== undefined) overlay.set(Boolean(value))
  },
  { immediate: true },
)
onScopeDispose(() => overlay.set(false))
</script>

<template>
  <NDrawer @update:show="onUpdateShow">
    <slot />
  </NDrawer>
</template>
