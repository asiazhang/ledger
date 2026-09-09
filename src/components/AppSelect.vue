<script setup lang="ts">
import { NSelect } from 'naive-ui'
import { useOverlayReporting } from '@/composables/useOverlayReporting'

// 薄封装 NSelect，接入弹层注册表（ADR-0035）：应用内的 NSelect 一律经本组件
// 使用，开/关状态实时上报，驱动快捷键抑制。其余 props/attrs/slots 原样透传。
//
// 上报与关闭通道（含系统返回桥接的「关最上层」出口）收口于 useOverlayReporting
// （issue #845）：刻意不声明 show prop（Vue 对可选 Boolean prop 的缺席值转型为
// false，声明后未传 show 的非受控用法会被变成「受控关闭」，下拉永远打不开）。
// 非受控用法经影子态绑定 :show（点击开合经 update:show 回写，行为不变），受控
// 用法跟随调用方 :show；关闭请求受控中继调用方监听器、非受控落影子态。
const { onUpdateShow, resolvedShow } = useOverlayReporting('select')
</script>

<template>
  <NSelect :show="resolvedShow" @update:show="onUpdateShow">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NSelect>
</template>
