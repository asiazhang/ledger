<script setup lang="ts">
import { NDropdown } from 'naive-ui'
import { useOverlayReporting } from '@/composables/useOverlayReporting'

// 薄封装 NDropdown，接入弹层注册表（ADR-0035）：应用内的 NDropdown 一律经本
// 组件使用，菜单开/关状态实时上报，驱动快捷键抑制。其余 props/attrs/slots
// 原样透传。
//
// 上报与关闭通道（含系统返回桥接的「关最上层」出口）收口于 useOverlayReporting
// （issue #845）：刻意不声明 show prop（先例 AppModal / AppSelect）——:show /
// @update:show 经 attrs 原样透传；非受控开合与 clickoutside/选中触发的关闭经根
// 上的 update:show 监听上报，trigger="manual" 下调用方直接改 :show prop 的开合
// 由 attrs watch 兜底。非受控用法经影子态绑定 :show，关闭请求受控中继调用方监
// 听器、非受控落影子态。
const { onUpdateShow, resolvedShow } = useOverlayReporting('dropdown')
</script>

<template>
  <NDropdown :show="resolvedShow" @update:show="onUpdateShow">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NDropdown>
</template>
