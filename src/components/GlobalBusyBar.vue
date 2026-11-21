<script setup lang="ts">
import { useThemeVars } from "naive-ui";
import { busyVisible } from "@ledger/api";
import { useWindowTier } from "@ledger/window-tier";
import { t } from "@ledger/i18n";

const themeVars = useThemeVars();

const tier = useWindowTier();
</script>

<template>
  <Transition name="global-busy-bar">
    <div
      v-if="busyVisible"
      class="global-busy-bar"
      :class="{ 'is-mobile-tier': tier === 'mobile' }"
      role="progressbar"
      :style="{ '--busy-color': themeVars.primaryColor }"
      :aria-label="t('common.globalBusyBar.label')"
    >
      <div class="global-busy-bar-thumb" />
    </div>
  </Transition>
</template>

<style scoped>
.global-busy-bar {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  height: 2px;
  z-index: 3000;
  pointer-events: none;
  overflow: hidden;
}

.global-busy-bar.is-mobile-tier {
  top: env(safe-area-inset-top, 0px);
}

.global-busy-bar-thumb {
  height: 100%;
  width: 30%;
  border-radius: 999px;
  background: var(--busy-color);
  animation: global-busy-bar-slide 1.4s ease-in-out infinite;
}

@keyframes global-busy-bar-slide {
  from {
    transform: translateX(-110%);
  }
  to {
    transform: translateX(400%);
  }
}

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
