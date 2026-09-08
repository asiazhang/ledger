<script setup lang="ts">
import { ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { NIcon, NMenu, useThemeVars } from 'naive-ui'
import type { MenuOption } from 'naive-ui'
import { MenuOutline } from '@vicons/ionicons5'
import { t } from '@/i18n'
import AppDrawer from '@/components/AppDrawer.vue'

/**
 * 移动档导航壳（issue #842 / ADR-0088 决策 4，词汇表「导航抽屉」）：
 * 顶部应用栏（汉堡按钮 + 当前视图名）+ 导航抽屉。桌面档布局一字不动（App.vue 按
 * 窗口分级分支），本组件只服务 <840 移动档。
 *
 * - **导航状态单一来源**：抽屉菜单消费调用方传入的 menuOptions（与桌面侧栏同一份
 *   派生、剥离键位带——移动档不渲染键位提示，ViewState 组内序/收纳清单语义零改动）；
 *   选中经 select 事件交调用方路由，本组件不持导航语义。
 * - **桌面专属管理动作不上抽屉**（ADR-0088 决策 10）：菜单不附右键排序/收纳管理
 *   （调用方 nodeProps 仅桌面档侧栏消费）。
 * - **抽屉是弹层**：经 AppDrawer 薄封装上报弹层注册表（ADR-0035），开合期间快捷键
 *   抑制判定照常；遮罩点击与 ESC 关闭（导航菜单无表单状态，误触零代价，非模态家族）。
 * - **触控目标 ≥48px**（ADR-0088 全局验收基线）：菜单行拉至 48px，「更多」链接与
 *   品牌行隐私眼睛按钮以伪元素扩展热区（视觉不变，命中面达标）。
 * - **导航即关闭**：监听路由变化关抽屉——菜单项与组标题行「更多」链接两类入口统一。
 * - **安全区**：顶栏 padding-top 避让状态栏/刘海，内容底部避让手势条
 *   （viewport-fit=cover 下 env 生效；桌面档 env 恒 0，零影响）。
 */
const props = defineProps<{
  /** 抽屉菜单选项：与桌面侧栏同一份派生（消费同一份导航状态） */
  menuOptions: MenuOption[]
  /** 当前视图名（顶栏标题；视图文案由调用方经 viewLabel 解析） */
  title: string
}>()

const emit = defineEmits<{ select: [key: string] }>()

const route = useRoute()
const drawerShow = ref(false)

// 导航即关闭：菜单项点击（select 内先关）与「更多」链接（路由跳转）统一收口
watch(
  () => route.fullPath,
  () => {
    drawerShow.value = false
  },
)

function onSelect(key: string) {
  drawerShow.value = false
  emit('select', key)
}

// 顶栏配色取自应用主题（亮暗即时换色）；触控目标 ≥48px（ADR-0088 全局验收基线）
const themeVars = useThemeVars()
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
      <span class="mobile-top-bar-title">{{ title }}</span>
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

/* 顶栏：安全区避让（viewport-fit=cover 下 env 生效，桌面档恒 0） */
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

/* 汉堡按钮：48x48 触控目标（ADR-0088 全局验收基线） */
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
  font-size: 18px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 内容区：自身滚动（移动档原生滚动），底部避让手势条 */
.mobile-content {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: none;
  padding: 16px;
  padding-bottom: calc(16px + env(safe-area-inset-bottom, 0px));
}

/* 抽屉体：品牌行 + 菜单整体滚动 */
.mobile-drawer-body {
  height: 100%;
  overflow-y: auto;
  overscroll-behavior: none;
}

/* 触控目标 ≥48px（ADR-0088 全局验收基线）：抽屉菜单行拉至 48px（桌面侧栏口径不变）。
   这些规则只作用于抽屉作用域（.mobile-drawer-body 后代），naive 自身行高规则以
   元素类选择器命中，此处带 [data-v] 祖先限定权重更高。 */
.mobile-drawer-body :deep(.n-menu-item) {
  height: 48px;
}

/* 「更多」链接与品牌行隐私眼睛按钮：视觉不变，以透明伪元素扩展命中面至 ≈48px。
   链接本体在组标题行内（拉高会破坏组行布局），伪元素外扩是热区适配的唯一不破坏面。 */
.mobile-drawer-body :deep(.group-more-link),
.mobile-drawer-body :deep(.n-button) {
  position: relative;
}

.mobile-drawer-body :deep(.group-more-link)::after,
.mobile-drawer-body :deep(.n-button)::after {
  content: '';
  position: absolute;
  inset: -16px -14px;
}
</style>
