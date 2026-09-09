<script setup lang="ts">
import { NDatePicker } from 'naive-ui'
import { useOverlayReporting } from '@/composables/useOverlayReporting'

// 薄封装 NDatePicker，接入弹层注册表（ADR-0035）：应用内的 NDatePicker 一律经
// 本组件使用，面板开/关状态实时上报，驱动快捷键抑制。其余 props/attrs/slots
// 原样透传。
//
// 上报与关闭通道（含系统返回桥接的「关最上层」出口）收口于 useOverlayReporting
// （issue #845）：刻意不声明 show prop（原因见 AppSelect 注释）——:show /
// @update:show 经 attrs 原样透传；面板开合经根上的 update:show 监听上报，受控
// 调用方直接改 :show prop 的开合由 attrs watch 兜底。非受控用法经影子态绑定
// :show，关闭请求受控中继调用方监听器、非受控落影子态。
const { onUpdateShow, resolvedShow } = useOverlayReporting('date-picker')
</script>

<template>
  <NDatePicker :show="resolvedShow" @update:show="onUpdateShow">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NDatePicker>
</template>
