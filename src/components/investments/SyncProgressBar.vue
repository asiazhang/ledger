<script setup lang="ts">
import { computed } from 'vue'
import { t } from '@/i18n'
import type { InstrumentSyncProgress } from '@/types'
import { bar, root, text, textStack, track } from './sync-progress-bar.css.ts'

// 同步进度条展示组件（issue #897 / ADR-0095）：标的信息同步的确定进度——
// 形态与 GlobalBusyBar 同款细条（2px），蓝色区分（品牌强调色为琥珀，蓝色是
// 同步进度专属色）；条旁一行计数文案「同步标的信息 37/100」经 i18n 随界面
// 语言（ADR-0049 文案纪律）。纯展示组件：进度状态由 useInstrumentInfoSync
// 接缝统一产出，两入口（盈亏页当前持仓卡 / 标的页标的列表）渲染同一份组件，
// 行为零分叉。位于卡片内文档流、非模态环境指示（不接弹层注册表、不抑制快捷
// 键，与 GlobalBusyBar 同一豁免口径）。
//
// 基金深回填的页级明细（issue #1061）：主行计数之下再渲染一行「回填 110022：
// 第 3/25 页」——单只基金首刷要翻约 25 页，明细让这段接近一分钟的窗口可见；
// 进度条宽度仍按标的级 done/total，页推进不虚报整体百分比。

const props = defineProps<{ progress: InstrumentSyncProgress | null }>()

/** 确定百分比：done/total；total 异常（≤0）时按 0 处理（不渲染态之外的双保险）。 */
const percent = computed(() => {
  if (!props.progress || props.progress.total <= 0) return 0
  return Math.min(100, Math.round((props.progress.done / props.progress.total) * 100))
})
</script>

<template>
  <div
    v-if="progress"
    :class="root"
    data-testid="instrument-sync-progress"
    role="progressbar"
    :aria-valuemin="0"
    :aria-valuemax="progress.total"
    :aria-valuenow="progress.done"
    :aria-label="t('investments.sync.progressAriaLabel')"
  >
    <div :class="track">
      <div :class="bar" data-testid="instrument-sync-progress-bar" :style="{ width: percent + '%' }" />
    </div>
    <div :class="textStack">
      <span :class="text">
        {{ t('investments.sync.progress', { done: progress.done, total: progress.total }) }}
      </span>
      <span v-if="progress.fund" :class="text" data-testid="instrument-sync-progress-fund">
        {{ t('investments.sync.progressFundPages', {
          code: progress.fund.code,
          page: progress.fund.page,
          pages: progress.fund.pages,
        }) }}
      </span>
    </div>
  </div>
</template>
