<script setup lang="ts">
import { NPopover } from 'naive-ui'
import { useOverlayReporting } from '@/composables/useOverlayReporting'

// 薄封装 NPopover，接入弹层注册表（ADR-0035）：应用内的 NPopover 一律经本
// 组件使用，弹层开/关状态实时上报，驱动快捷键抑制。default（内容）/trigger
// 等 slots 与其余 props/attrs 原样透传。
//
// 上报与关闭通道（含系统返回桥接的「关最上层」出口）收口于 useOverlayReporting
// （issue #845）：刻意不声明 show prop（原因见 AppSelect 注释）——:show /
// @update:show 经 attrs 原样透传；非受控开合与 clickoutside 触发的关闭
//（trigger="click" 等）经根上的 update:show 监听上报，受控调用方直接改 :show
// prop 的开合由 attrs watch 兜底。非受控用法经影子态绑定 :show，关闭请求受控
// 中继调用方监听器、非受控落影子态。先例：账本入口弹层（issue #834）、交易表
// 金额全文点按查看（ADR-0088 决策 6 悬停一击可达）。
const { onUpdateShow, resolvedShow } = useOverlayReporting('popover')
</script>

<template>
  <NPopover :show="resolvedShow" @update:show="onUpdateShow">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NPopover>
</template>
