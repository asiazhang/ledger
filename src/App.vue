<script setup lang="ts">
import { computed, h, nextTick, ref, type Component, type HTMLAttributes } from 'vue'
import { RouterView, useRouter, useRoute } from 'vue-router'
import {
  NConfigProvider,
  NMessageProvider,
  NDialogProvider,
  enUS,
  dateEnUS,
  NLayout,
  NLayoutSider,
  NLayoutContent,
  NMenu,
  NIcon,
  NSpace,
  NText,
  darkTheme,
  zhCN,
  dateZhCN,
  type MenuOption,
} from 'naive-ui'
import AppDropdown from '@/components/AppDropdown.vue'
import {
  HomeOutline,
  SwapHorizontalOutline,
  SearchOutline,
  WalletOutline,
  BarChartOutline,
  TrendingUpOutline,
  CubeOutline,
  RepeatOutline,
  CalculatorOutline,
  SparklesOutline,
  SettingsOutline,
} from '@vicons/ionicons5'
import { useAppStore } from '@/stores/app'
import { currentLocale } from '@/i18n'
import { darkOverrides, lightOverrides } from '@/theme/overrides'
import { useDevicePreferenceSync } from '@/composables/useDevicePreferenceSync'
import MessageSinkBridge from '@/components/MessageSinkBridge.vue'
import { loadSidebarCollapsed, saveSidebarCollapsed } from '@/utils/view-state'
import {
  viewShortcuts,
  shortcutHint,
  useViewShortcuts,
  isArrangeableView,
  isSidebarSortAction,
  sidebarOrder,
  buildSidebarSortMenuOptions,
  applySidebarSort,
  resetSidebarOrder,
  type ViewName,
} from '@/composables/useViewShortcuts'
import { useWindowGuard } from '@/composables/useWindowGuard'

const router = useRouter()
const route = useRoute()

// 窗口行为守卫（issue #154）：ESC 不作用于窗口层 + 禁用原生右键菜单（可编辑元素例外），
// 根组件挂载一次，详见 composables/useWindowGuard.ts。
useWindowGuard()

// ViewState：侧边栏折叠状态跨启动保持。
const sidebarCollapsed = ref(loadSidebarCollapsed())
function updateSidebarCollapsed(collapsed: boolean) {
  sidebarCollapsed.value = collapsed
  saveSidebarCollapsed(collapsed)
}
const store = useAppStore()

// UI 组件库内置文案（日期选择器、分页、空态等）随应用界面语言切换（ADR-0048）：
// 经 NConfigProvider 的 locale / date-locale 注入，语言切换即时生效。
const naiveLocale = computed(() => (currentLocale.value === 'en-US' ? enUS : zhCN))
const naiveDateLocale = computed(() => (currentLocale.value === 'en-US' ? dateEnUS : dateZhCN))

// 设备偏好镜像推送（备份目录 issue #125 / ADR-0016；自动执行开关 issue #308 / ADR-0042）：
// 真源在前端 localStorage（应用设置 store），启动/变更时把镜像推给后端运行时消费。
useDevicePreferenceSync()

const viewLabels: Record<string, string> = {
  dashboard: '概览',
  transactions: '交易',
  search: '搜索',
  accounts: '账户',
  reports: '报表',
  investments: '投资',
  items: '物品',
  scheduled: '定时',
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
  items: CubeOutline,
  scheduled: RepeatOutline,
  budget: CalculatorOutline,
  ai: SparklesOutline,
  settings: SettingsOutline,
}

function renderMenuIcon(name: string) {
  return () => h(NIcon, { size: 18 }, { default: () => h(viewIcons[name]) })
}

// 菜单项与快捷键共用同一顺序源（viewShortcuts：由最终序按位置推导键位）；
// 菜单由最终序响应式派生，排序变更时顺序与快捷键提示同步更新。
// 每项右侧附快捷键提示（数字位或设置项的 ⌘,）；
// 可排区八项经 nodeProps 附右键排序菜单（issue #270），三固定项不附（右键无任何菜单，
// 原生菜单由窗口守卫抑制）。注：NMenu 不支持选项级 props 字段，必须走菜单级 nodeProps。
const menuOptions = computed<MenuOption[]>(() =>
  viewShortcuts.value.map(({ name, key }) => ({
    key: name,
    icon: renderMenuIcon(name),
    label: () =>
      h('div', { style: 'display:flex;justify-content:space-between;align-items:center;gap:12px;padding-right:2px' }, [
        h('span', viewLabels[name]),
        h('span', { style: 'font-size:12px;opacity:.55' }, shortcutHint(key)),
      ]),
  })),
)

/** 菜单级 nodeProps：仅可排区项附右键事件（固定项无任何右键菜单）。
 *  naive-ui 的 nodeProps 返回类型把索引签名限成 string|number，事件函数过不去
 *  （类型仅对 data-* 友好），运行时照常铺到节点上，故此处断言放宽。 */
function nodeProps(option: MenuOption) {
  const name = option.key as string
  if (!isArrangeableView(name)) return {}
  return {
    onContextmenu: (e: MouseEvent) => showSortMenu(e, name),
  } as unknown as HTMLAttributes & Record<string, string | number | undefined>
}

// ---------------------------------------------------------------------------
// 侧栏右键排序菜单（issue #270）：可排区八项右键弹出（上移/下移/移顶/移底/恢复默认），
// 手动定位弹出，与行级右键菜单同一模式；点选即重排并立即持久化，
// 菜单打开期间视图快捷键由既有弹层抑制机制压制（零新代码）。
// ---------------------------------------------------------------------------

const sortMenuShow = ref(false)
const sortMenuX = ref(0)
const sortMenuY = ref(0)
const sortTarget = ref<ViewName | null>(null)

const sortMenuOptions = computed(() =>
  sortTarget.value ? buildSidebarSortMenuOptions(sortTarget.value, sidebarOrder.value) : [],
)

/** 右键可排区项弹出排序菜单：先收起再 nextTick 展开，保证连续弹出时位置刷新。 */
function showSortMenu(e: MouseEvent, name: ViewName) {
  sortTarget.value = name
  sortMenuX.value = e.clientX
  sortMenuY.value = e.clientY
  sortMenuShow.value = false
  void nextTick(() => {
    sortMenuShow.value = true
  })
}

function onSortMenuSelect(key: string) {
  sortMenuShow.value = false
  const target = sortTarget.value
  if (!target) return
  if (key === 'reset') {
    resetSidebarOrder()
    return
  }
  // 菜单 key 与移动动作同一词表（key 即 action），守卫收窄后零断言
  if (isSidebarSortAction(key)) {
    applySidebarSort(target, key)
  }
}

// 视图快捷键：窗口内 Cmd/Ctrl+1..0 与 Cmd/Ctrl+, 切换视图（弹窗/确认框打开时自动抑制）
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
    :locale="naiveLocale"
    :date-locale="naiveDateLocale"
  >
    <NMessageProvider>
      <NDialogProvider>
        <!-- Loadable toast sink 注册桥（ADR-0040）：必须在消息提供器子树内 -->
        <MessageSinkBridge />
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
                :node-props="nodeProps"
                @update:value="handleSelect"
              />
            </NSpace>
            <!-- 可排区右键排序菜单（issue #270）：手动定位弹出 -->
            <AppDropdown
              trigger="manual"
              placement="bottom-start"
              :show="sortMenuShow"
              :x="sortMenuX"
              :y="sortMenuY"
              :options="sortMenuOptions"
              :min-width="140"
              @select="onSortMenuSelect"
              @clickoutside="sortMenuShow = false"
            />
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
