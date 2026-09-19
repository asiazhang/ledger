<script setup lang="ts">
import { NText } from "naive-ui";
import { t } from "@ledger/i18n";

// 同步降级标注组件（issue #1376 / ADR-0121 决策 4）：本次同步回退到逐标的通道
// （批量取数面失败或跨同步停用期）时明示「已降级、本次较慢」——偶发的「这次
// 同步特别慢」由此有明确解释，不静默变慢（公开发布的应用里，静默降级会变成
// 无法排查的缺陷报告）。纯展示组件：降级事实由 useInstrumentInfoSync 接缝随
// 同步结果产出，两入口（盈亏页当前持仓卡 / 标的页标的列表）渲染同一份组件，
// 行为零分叉。与结果消息同生命周期（新一次同步开始即收起）；正常（批量面
// 命中）路径不渲染。位于卡片内文档流、非模态环境指示（不接弹层注册表、不
// 抑制快捷键，与 SyncProgressBar 同一豁免口径）；着色用警示色（warning）与
// 成功/失败消息的 info/error 同族，不新造样式（无独立样式文件）。

defineProps<{ degraded: boolean }>();
</script>

<template>
  <NText v-if="degraded" type="warning" data-testid="instrument-sync-degraded">
    {{ t("investments.sync.degraded") }}
  </NText>
</template>
