<script setup lang="ts">
import { NDrawer } from 'naive-ui'
import { useOverlayReporting } from '@/composables/useOverlayReporting'

// AppDrawer（issue #842 / ADR-0088 决策 4）：薄封装 NDrawer——导航抽屉（移动档壳层，
// 词汇表「导航抽屉」）的唯一使用形态，接入弹层注册表（ADR-0035）：开/关状态实时上报，
// 与其他弹层封装共同驱动快捷键抑制的同一判定闸门。其余 props/attrs 原样透传
// （透传依赖单根节点 attrs fallthrough）。
//
// 上报与关闭通道（含系统返回桥接的「关最上层」出口）收口于 useOverlayReporting
// （issue #845）：刻意不声明 show prop（先例 AppModal / AppSelect）——受控调用方
// 经 v-model:show 的开合由根上的 update:show 监听上报，直接改 :show prop 的开合
// 由 attrs watch 兕底；关闭请求中继调用方监听器。卸载兜底（作用域销毁时撤销上报，
// 幂等）也在该机制点内：跨断点换档（窗口分级，ADR-0088 决策 2）时移动壳整体卸载，
// 抽屉可能仍开着——NDrawer 是受控组件，卸载不产生 update:show(false)，注册表会
// 滞留开放态导致快捷键永久抑制。
const { onUpdateShow, resolvedShow } = useOverlayReporting('drawer')
</script>

<template>
  <NDrawer :show="resolvedShow" @update:show="onUpdateShow">
    <slot />
  </NDrawer>
</template>
