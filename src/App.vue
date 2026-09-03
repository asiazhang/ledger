<script setup lang="ts">
import { computed, h, nextTick, ref, watch, type Component, type HTMLAttributes } from 'vue'
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
  ShieldCheckmarkOutline,
  RepeatOutline,
  CalculatorOutline,
  EllipsisHorizontalOutline,
  SparklesOutline,
  SettingsOutline,
} from '@vicons/ionicons5'
import { useAppStore } from '@/stores/app'
import { currentLocale, t } from '@/i18n'
import { viewLabel } from '@/i18n/view-label'
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
  sidebarGroups,
  sidebarGroupOrders,
  sidebarContainment,
  groupOfView,
  buildSidebarSortMenuOptions,
  applySidebarSort,
  resetSidebarOrder,
  FIRST_VIEW,
  PENULTIMATE_VIEW,
  LAST_VIEW,
  EXTRA_VIEW,
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

// UI 组件库内置文案（日期选择器、分页、空态等）随应用界面语言切换（ADR-0049）：
// 经 NConfigProvider 的 locale / date-locale 注入，语言切换即时生效。
// 窗口标题随界面语言（原生窗口壳层文案）；非 Tauri 环境（测试/Web）静默忽略
watch(
  currentLocale,
  async () => {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      void getCurrentWindow().setTitle(t('common.window.title'))
    } catch {
      /* 非 Tauri 环境 */
    }
  },
  { immediate: true },
)

const naiveLocale = computed(() => (currentLocale.value === 'en-US' ? enUS : zhCN))
const naiveDateLocale = computed(() => (currentLocale.value === 'en-US' ? dateEnUS : dateZhCN))

// 设备偏好镜像推送（备份目录 issue #125 / ADR-0016；自动执行开关 issue #308 / ADR-0042）：
// 真源在前端 localStorage（应用设置 store），启动/变更时把镜像推给后端运行时消费。
useDevicePreferenceSync()

// 视图名称走文案资源（issue #342）：侧栏菜单与内容区标题同源，随界面语言即时切换；
// key 构造收口在 i18n/view-label（key 契约有单测，漏域名前缀会原样渲染 key 代号）。

// 视图图标（ionicons5 Outline 风格，与分类图标一致）
const viewIcons: Record<string, Component> = {
  dashboard: HomeOutline,
  transactions: SwapHorizontalOutline,
  search: SearchOutline,
  accounts: WalletOutline,
  reports: BarChartOutline,
  investments: TrendingUpOutline,
  items: CubeOutline,
  policies: ShieldCheckmarkOutline,
  scheduled: RepeatOutline,
  more: EllipsisHorizontalOutline,
  budget: CalculatorOutline,
  ai: SparklesOutline,
  settings: SettingsOutline,
}

function renderMenuIcon(name: string) {
  return () => h(NIcon, { size: 18 }, { default: () => h(viewIcons[name]) })
}

// 菜单形态（issue #359 侧栏分组；#372 增「更多」第四固定项）：概览（固定）+
// 记账/资产/洞察三组 + 更多、AI、设置（三固定项）。菜单项与快捷键共用同一顺序源
// （viewShortcuts：由组内序按线性位置推导键位），分组标题不占键位、不参与排序与计数
// （NMenu group 选项天然不可选）；菜单响应式派生，组内排序变更时顺序与快捷键提示同步更新。
// 每项右侧附快捷键提示（数字位或设置项的 ⌘,）；「更多」与概览/AI/设置同属固定项，无键位不出提示。
// 可排区八项经 nodeProps 附右键组内排序菜单（issue #270/#359），固定项与分组标题不附
// （右键无任何菜单，原生菜单由窗口守卫抑制）。注：NMenu 不支持选项级 props 字段，必须走菜单级 nodeProps。
function renderItem(name: ViewName, key: string | null): MenuOption {
  return {
    key: name,
    icon: renderMenuIcon(name),
    label: () =>
      h('div', { style: 'display:flex;justify-content:space-between;align-items:center;gap:12px;padding-right:2px' }, [
        h('span', viewLabel(name)),
        key === null ? null : h('span', { style: 'font-size:12px;opacity:.55' }, shortcutHint(key)),
      ]),
  }
}

const menuOptions = computed<MenuOption[]>(() => {
  const keyOf = new Map(viewShortcuts.value.map((s) => [s.name, s.key]))
  const item = (name: ViewName) => renderItem(name, keyOf.get(name) ?? null)
  return [
    item(FIRST_VIEW),
    ...sidebarGroups.value.map((g): MenuOption => ({
      type: 'group',
      key: `sidebar-group:${g.id}`,
      // 组标题行：组名 + 按需「更多」链接（issue #472 / ADR-0063 决策 1）——
      // 链接仅当组内存在收纳成员时渲染（菜单分组标题天然不可交互，链接是自定义渲染的新元素）；
      // 折叠态不渲染组标题，链接随之不渲染（决策 6 天然成立）；链接无键位、不出提示。
      label: () =>
        h('div', { class: 'sidebar-group-title' }, [
          h('span', t(`common.sidebarGroup.${g.id}`)),
          sidebarContainment.value[g.id].length > 0
            ? h('a', { class: 'group-more-link', onClick: () => { void router.push({ name: `${g.id}-more` }) } }, t('common.nav.more'))
            : null,
        ]),
      children: g.views.map((name) => item(name)),
    })),
    item(EXTRA_VIEW),
    item(PENULTIMATE_VIEW),
    item(LAST_VIEW),
  ]
})

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
// 侧栏右键排序菜单（issue #270，#359 收窄为组内）：可排区八项右键弹出组内排序菜单
// （上移/下移/移顶/移底/恢复默认），手动定位弹出，与行级右键菜单同一模式；
// 点选即重排并立即持久化，菜单打开期间视图快捷键由既有弹层抑制机制压制（零新代码）。
// ---------------------------------------------------------------------------

const sortMenuShow = ref(false)
const sortMenuX = ref(0)
const sortMenuY = ref(0)
const sortTarget = ref<ViewName | null>(null)

const sortMenuOptions = computed(() => {
  const target = sortTarget.value
  const gid = target ? groupOfView(target) : null
  if (!target || !gid) return []
  return buildSidebarSortMenuOptions(target, sidebarGroupOrders.value[gid])
})

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

const title = () => h('div', { style: 'padding: 16px; font-size: 18px; font-weight: 600' }, '📒 Ledger')

const pageTitle = computed(() => (typeof route.name === 'string' ? viewLabel(route.name) : ''))
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
                :indent="16"
                :node-props="nodeProps"
                @update:value="handleSelect"
              />
            </NSpace>
            <!-- 可排区右键组内排序菜单（issue #270/#359）：手动定位弹出 -->
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
                  {{ pageTitle }}
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

<style scoped>
/* 组标题行「更多」链接（issue #472 / ADR-0063 决策 1）：标题行两端对齐，
   链接弱化为次级样式，点击进入该组「更多」聚合页；无键位提示。 */
.sidebar-group-title {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
}

.group-more-link {
  font-size: 12px;
  opacity: 0.6;
  cursor: pointer;
}

.group-more-link:hover {
  opacity: 1;
}
</style>
