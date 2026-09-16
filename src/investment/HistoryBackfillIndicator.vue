<script setup lang="ts">
import { t } from '@ledger/i18n'
import { root, text } from './history-backfill-indicator.css.ts'
import { useHistoryBackfill } from './useHistoryBackfill'

// 价格历史后台补全的静默计数（issue #1375 / ADR-0122）：投资页内唯一的
// 用户可见面——「历史数据补全中 137/228」这类只读计数。刻意不做的事：
// 不占全局忙碌条、不改变任何页面可用性、不可点（pointer-events 关闭）、
// 无进度条形态（那是手动同步的专属形态）、终态（done ≥ total）静默收起
//（收起逻辑归 useHistoryBackfill 接缝，本组件纯展示）。
//
// 基金首刷深回填的页级明细（issue #1061 形状随迁）：主行计数之下再渲染
// 一行「回填 110022：第 3/25 页」。

const { progress } = useHistoryBackfill()
</script>

<template>
  <div
    v-if="progress"
    :class="root"
    data-testid="history-backfill-indicator"
  >
    <span :class="text">
      {{ t('investments.backfill.progress', { done: progress.done, total: progress.total }) }}
    </span>
    <span v-if="progress.fund" :class="text" data-testid="history-backfill-indicator-fund">
      {{ t('investments.backfill.progressFundPages', {
        code: progress.fund.code,
        page: progress.fund.page,
        pages: progress.fund.pages,
      }) }}
    </span>
  </div>
</template>
