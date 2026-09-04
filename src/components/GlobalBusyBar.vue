<script setup lang="ts">
// 全局忙碌条（issue #500 / spec #498）：窗口顶部细条，无确定进度（indeterminate）。
// 只做渲染消费——可见性唯一来源是忙碌状态模块的 busyVisible，聚合/阈值/递减语义
// 全部收口在模块侧；本组件不持状态、不接弹层注册表（非模态环境指示，不抑制快捷键，
// ADR-0035 豁免，理由见词汇表 GlobalBusyBar 词条）。无障碍标签经 i18n（ADR-0049）。
import { useThemeVars } from 'naive-ui'
import { busyVisible } from '@/composables/globalBusy'
import { t } from '@/i18n'

// 强调色取自应用主题（useThemeVars 需在 NConfigProvider 子树内），亮暗主题即时换色
const themeVars = useThemeVars()
</script>

<template>
  <Transition name="global-busy-bar">
    <div
      v-if="busyVisible"
      class="global-busy-bar"
      role="progressbar"
      :style="{ '--busy-color': themeVars.primaryColor }"
      :aria-label="t('common.globalBusyBar.label')"
    >
      <div class="global-busy-bar-thumb" />
    </div>
  </Transition>
</template>

<style scoped>
/* 固定在窗口最顶部的一条细带：不占布局、不拦截任何指针事件（环境指示不抢交互） */
.global-busy-bar {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  height: 3px;
  z-index: 3000;
  pointer-events: none;
  overflow: hidden;
}

/* 无确定进度的往返滑动：只表达「在工作」，不表达进度 */
.global-busy-bar-thumb {
  height: 100%;
  width: 30%;
  border-radius: 999px;
  background: var(--busy-color);
  animation: global-busy-bar-slide 1.1s ease-in-out infinite;
}

@keyframes global-busy-bar-slide {
  from {
    transform: translateX(-110%);
  }
  to {
    transform: translateX(400%);
  }
}

/* 减弱动态偏好：退化为整条静置显示，保留「忙碌」信号本身 */
@media (prefers-reduced-motion: reduce) {
  .global-busy-bar-thumb {
    width: 100%;
    animation: none;
    opacity: 0.55;
  }
}

.global-busy-bar-enter-active,
.global-busy-bar-leave-active {
  transition: opacity 0.2s ease;
}

.global-busy-bar-enter-from,
.global-busy-bar-leave-to {
  opacity: 0;
}
</style>
