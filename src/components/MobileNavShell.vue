<script setup lang="ts">
import { ref, watch } from "vue";
import { useRoute } from "vue-router";
import { NIcon, NMenu, useThemeVars } from "naive-ui";
import type { MenuOption } from "naive-ui";
import { MenuOutline } from "@vicons/ionicons5";
import { t } from "@ledger/i18n";
import AppDrawer from "@ledger/ui-kit/AppDrawer.vue";

const props = defineProps<{
  menuOptions: MenuOption[];
  title: string;
}>();

const emit = defineEmits<{ select: [key: string] }>();

const route = useRoute();
const drawerShow = ref(false);

watch(
  () => route.fullPath,
  () => {
    drawerShow.value = false;
  },
);

function onSelect(key: string) {
  drawerShow.value = false;
  emit("select", key);
}

const themeVars = useThemeVars();
</script>

<template>
  <div class="mobile-shell">
    <header
      class="mobile-top-bar"
      :style="{
        '--top-bar-bg': themeVars.bodyColor,
        '--top-bar-border': themeVars.dividerColor,
        '--top-bar-hover': themeVars.hoverColor,
      }"
    >
      <button
        type="button"
        class="mobile-hamburger"
        :aria-label="t('common.nav.openDrawer')"
        @click="drawerShow = true"
      >
        <NIcon :size="24"><MenuOutline /></NIcon>
      </button>
      <h1 class="mobile-top-bar-title">{{ title }}</h1>
    </header>
    <div class="mobile-content">
      <slot />
    </div>
    <AppDrawer
      v-model:show="drawerShow"
      placement="left"
      :width="280"
      :mask-closable="true"
      content-style="padding: 0;"
    >
      <div class="mobile-drawer-body">
        <slot name="brand" />
        <NMenu
          :options="menuOptions"
          :value="route.name as string"
          :indent="16"
          @update:value="onSelect"
        />
      </div>
    </AppDrawer>
  </div>
</template>

<style scoped>
.mobile-shell {
  display: flex;
  flex-direction: column;
  height: 100vh;
}

.mobile-top-bar {
  flex: none;
  display: flex;
  align-items: center;
  gap: 4px;
  height: calc(56px + env(safe-area-inset-top, 0px));
  padding: env(safe-area-inset-top, 0px) 12px 0 4px;
  background: var(--top-bar-bg);
  border-bottom: 1px solid var(--top-bar-border);
}

.mobile-hamburger {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 48px;
  height: 48px;
  border: none;
  border-radius: 6px;
  padding: 0;
  background: transparent;
  color: inherit;
  cursor: pointer;
}

.mobile-hamburger:hover {
  background: var(--top-bar-hover);
}

.mobile-top-bar-title {
  flex: 1;
  min-width: 0;
  margin: 0;
  font-size: 18px;
  font-weight: 600;
  line-height: 1.4;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mobile-content {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: none;
  padding: 16px;
  padding-bottom: calc(16px + env(safe-area-inset-bottom, 0px));
}

.mobile-drawer-body {
  height: 100%;
  overflow-y: auto;
  overscroll-behavior: none;
}

.mobile-drawer-body :deep(.n-menu-item) {
  height: 48px;
}

.mobile-drawer-body :deep(.group-more-link),
.mobile-drawer-body :deep(.n-button) {
  position: relative;
}

.mobile-drawer-body :deep(.group-more-link)::after,
.mobile-drawer-body :deep(.n-button)::after {
  content: "";
  position: absolute;
  inset: -16px -14px;
}
</style>
