<script setup lang="ts">
// Loadable toast sink 接线桥（ADR-0040）：useMessage 只在 NMessageProvider 的
// 组件上下文内可用，应用入口经本桥在提供器内部把消息 API 注册为模块级单点 sink。
// 注册前模块 sink 为 no-op；本桥必须挂在 NMessageProvider 子树内（见 App.vue）。
import { useMessage } from 'naive-ui'
import { registerToastSink } from '@/composables/useLoadable'

registerToastSink(useMessage())
</script>

<template></template>
