<script setup lang="ts">
import { computed } from 'vue'
import { t } from '@/i18n'
import type { InstrumentSyncProgress } from '@/types'

// 同步进度条展示组件（issue #897 / ADR-0095）：标的信息同步的确定进度——
// 形态与 GlobalBusyBar 同款细条（2px），蓝色区分（品牌强调色为琥珀，蓝色是
// 同步进度专属色）；条旁一行计数文案「同步标的信息 37/100」经 i18n 随界面
// 语言（ADR-0049 文案纪律）。纯展示组件：进度状态由 useInstrumentInfoSync
// 接缝统一产出，两入口（盈亏页当前持仓卡 / 标的页标的列表）渲染同一份组件，
// 行为零分叉。位于卡片内文档流、非模态环境指示（不接弹层注册表、不抑制快捷
// 键，与 GlobalBusyBar 同一豁免口径）。

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
    class="sync-progress"
    data-testid="instrument-sync-progress"
    role="progressbar"
    :aria-valuemin="0"
    :aria-valuemax="progress.total"
    :aria-valuenow="progress.done"
    :aria-label="t('investments.sync.progressAriaLabel')"
  >
    <div class="sync-progress-track">
      <div class="sync-progress-bar" :style="{ width: percent + '%' }" />
    </div>
    <span class="sync-progress-text">
      {{ t('investments.sync.progress', { done: progress.done, total: progress.total }) }}
    </span>
  </div>
</template>

<style scoped>
/* 与 GlobalBusyBar 同款细条高度（2px）；占据文档流一行，不拦截指针 */
.sync-progress {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  pointer-events: none;
}

.sync-progress-track {
  flex: 1;
  height: 2px;
  border-radius: 999px;
  background: rgba(127, 127, 127, 0.2);
  overflow: hidden;
}

/* 蓝色区分：同步进度专属色（亮暗主题同色相可读） */
.sync-progress-bar {
  height: 100%;
  border-radius: 999px;
  background: #2080f0;
  transition: width 0.2s ease;
}

/* 减弱动态偏好：宽度变化即时生效，不做过渡动画 */
@media (prefers-reduced-motion: reduce) {
  .sync-progress-bar {
    transition: none;
  }
}

.sync-progress-text {
  font-size: 12px;
  line-height: 1.4;
  opacity: 0.75;
  white-space: nowrap;
}
</style>
