<script setup lang="ts">
import { h, ref, type Component } from 'vue'
import { RouterView, useRouter, useRoute } from 'vue-router'
import {
  NConfigProvider,
  NMessageProvider,
  NDialogProvider,
  NLayout,
  NLayoutSider,
  NLayoutContent,
  NMenu,
  NIcon,
  NSpace,
  NText,
  darkTheme,
  type MenuOption,
} from 'naive-ui'
import {
  HomeOutline,
  SwapHorizontalOutline,
  SearchOutline,
  WalletOutline,
  BarChartOutline,
  TrendingUpOutline,
  CalculatorOutline,
  SparklesOutline,
  SettingsOutline,
} from '@vicons/ionicons5'
import { useAppStore } from '@/stores/app'
import { darkOverrides, lightOverrides } from '@/theme/overrides'
import { loadSidebarCollapsed, saveSidebarCollapsed } from '@/utils/viewState'
import { viewShortcuts, shortcutHint, useViewShortcuts } from '@/composables/useViewShortcuts'

const router = useRouter()
const route = useRoute()

// ViewState：侧边栏折叠状态跨启动保持。
const sidebarCollapsed = ref(loadSidebarCollapsed())
function updateSidebarCollapsed(collapsed: boolean) {
  sidebarCollapsed.value = collapsed
  saveSidebarCollapsed(collapsed)
}
const store = useAppStore()

const viewLabels: Record<string, string> = {
  dashboard: '概览',
  transactions: '交易',
  search: '搜索',
  accounts: '账户',
  reports: '报表',
  investments: '投资',
  budget: '预算',
  ai: 'AI',
  settings: '设置',
}

// 视图图标（ionicons5 Outline 风格，与分类图标一致）
const viewIcons: Record<string, Component> = {
  dashboard: HomeOutline,
  transactions: SwapHorizontalOutline,
  search: SearchOutline,
  accounts: WalletOutline,
  reports: BarChartOutline,
  investments: TrendingUpOutline,
  budget: CalculatorOutline,
  ai: SparklesOutline,
  settings: SettingsOutline,
}

function renderMenuIcon(name: string) {
  return () => h(NIcon, { size: 18 }, { default: () => h(viewIcons[name]) })
}

// 菜单项与快捷键共用同一映射（顺序 = Cmd/Ctrl+1..9），label 右侧附快捷键提示
const menuOptions: MenuOption[] = viewShortcuts.map(({ name, key }) => ({
  key: name,
  icon: renderMenuIcon(name),
  label: () =>
    h('div', { style: 'display:flex;justify-content:space-between;align-items:center;gap:12px;padding-right:2px' }, [
      h('span', viewLabels[name]),
      h('span', { style: 'font-size:12px;opacity:.55' }, shortcutHint(key)),
    ]),
}))

// 视图快捷键：窗口内 Cmd/Ctrl+1..9 切换视图（弹窗/确认框打开时自动抑制）
useViewShortcuts(router)

function handleSelect(key: string) {
  router.push({ name: key })
}

const title = () => h('div', { style: 'padding: 16px 18px; font-size: 18px; font-weight: 600' }, '📒 Ledger')
</script>

<template>
  <NConfigProvider
    :theme="store.theme === 'dark' ? darkTheme : null"
    :theme-overrides="store.theme === 'dark' ? darkOverrides : lightOverrides"
  >
    <NMessageProvider>
      <NDialogProvider>
        <NLayout has-sider style="height: 100vh">
          <NLayoutSider
            bordered
            :width="160"
            :collapsed="sidebarCollapsed"
            :collapsed-width="0"
            show-trigger="arrow-circle"
            collapse-mode="width"
            @update:collapsed="updateSidebarCollapsed"
          >
            <NSpace vertical :size="0">
              <component :is="title" />
              <NMenu
                :options="menuOptions"
                :value="route.name as string"
                @update:value="handleSelect"
              />
            </NSpace>
          </NLayoutSider>
          <NLayout>
            <NLayoutContent content-style="padding: 20px;" :native-scrollbar="false">
              <NSpace vertical :size="16">
                <NText strong style="font-size: 20px">
                  {{ (route.meta.title as string) ?? '' }}
                </NText>
                <RouterView />
              </NSpace>
            </NLayoutContent>
          </NLayout>
        </NLayout>
      </NDialogProvider>
    </NMessageProvider>
  </NConfigProvider>
</template>
