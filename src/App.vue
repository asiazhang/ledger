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
  NButton,
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
  CalculatorOutline,
  ChevronForwardOutline,
  LayersOutline,
  SparklesOutline,
  SettingsOutline,
  RepeatOutline,
  StorefrontOutline,
  ShieldCheckmarkOutline,
  EyeOutline,
  EyeOffOutline,
} from '@vicons/ionicons5'
import { useAppStore } from '@/stores/app'
import { currentLocale, t } from '@/i18n'
import { viewLabel } from '@/i18n/view-label'
import { darkOverrides, lightOverrides } from '@/theme/overrides'
import { useDevicePreferenceSync } from '@/composables/useDevicePreferenceSync'
import MessageSinkBridge from '@/components/MessageSinkBridge.vue'
import GlobalBusyBar from '@/components/GlobalBusyBar.vue'
import { loadSidebarCollapsed, saveSidebarCollapsed } from '@/utils/view-state'
import {
  viewShortcuts,
  shortcutHint,
  useViewShortcuts,
  isSidebarSortAction,
  sidebarGroups,
  sidebarGroupOrders,
  sidebarContainment,
  groupOfView,
  buildSidebarSortMenuOptions,
  applySidebarSort,
  applyMoveIntoMore,
  isSidebarMember,
  resetSidebarOrder,
  FIRST_VIEW,
  PENULTIMATE_VIEW,
  LAST_VIEW,
  type ViewName,
  type ContainableViewName,
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
      void getCurrentWindow().setTitle(t('common.app.name'))
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

// 视图图标（ionicons5 Outline 风格，与分类图标一致）：仅侧栏菜单项（主项 + 固定项）；
// 出厂收纳成员图标与 GroupMoreView 页签映射同源（#475：移回侧栏后以主项身份入侧栏，随选随用）。
const viewIcons: Record<string, Component> = {
  dashboard: HomeOutline,
  transactions: SwapHorizontalOutline,
  search: SearchOutline,
  accounts: WalletOutline,
  reports: BarChartOutline,
  investments: TrendingUpOutline,
  items: CubeOutline,
  budget: CalculatorOutline,
  ai: SparklesOutline,
  settings: SettingsOutline,
  scheduled: RepeatOutline,
  merchants: StorefrontOutline,
  policies: ShieldCheckmarkOutline,
  physicalAssets: CubeOutline,
}

function renderMenuIcon(name: string) {
  return () => h(NIcon, { size: 18 }, { default: () => h(viewIcons[name]) })
}

// 菜单形态（issue #359 侧栏分组；#473 终态，ADR-0063 决策 1）：概览（固定）+
// 记账/资产/洞察三组（各自主项 + 组标题行按需「更多」链接）+ AI、设置（两固定项）。
// 全局「更多」固定项已退役；菜单项与快捷键共用同一顺序源（viewShortcuts：由组内序按
// 线性位置推导键位，只扫主项），分组标题不占键位、不参与排序与计数
// （NMenu group 选项天然不可选）；菜单响应式派生，组内排序变更时顺序与快捷键提示同步更新。
// 每项右侧附快捷键提示（数字位或设置项的 ⌘,）；「更多」链接与收纳成员无键位、不出提示、
// 不可键盘触发；折叠态不渲染组标题，「更多」链接随之不渲染（决策 6）。
// 主项经 nodeProps 附右键组内排序菜单（issue #270/#359），固定项与分组标题不附
// （右键无任何菜单，原生菜单由窗口守卫抑制）。注：NMenu 不支持选项级 props 字段，必须走菜单级 nodeProps。
function renderItem(name: ViewName | ContainableViewName, key: string | null): MenuOption {
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
  // 入参含移回的种子成员（#475）：侧栏菜单项词表 = 主项 ∪ 出厂种子
  const item = (name: ViewName | ContainableViewName) => renderItem(name, keyOf.get(name) ?? null)
  return [
    item(FIRST_VIEW),
    ...sidebarGroups.value.map((g): MenuOption => ({
      type: 'group',
      key: `sidebar-group:${g.id}`,
      // 组标题行：组名 + 按需「更多」链接（issue #472/#473 / ADR-0063 决策 1）——
      // 链接仅当组内存在收纳成员时渲染（菜单分组标题天然不可交互，链接是自定义渲染的新元素）；
      // 折叠态不渲染组标题，链接随之不渲染（决策 6 天然成立）；链接无键位、不出快捷键提示。
      label: () =>
        h('div', { class: 'sidebar-group-title' }, [
          h('span', t(`common.sidebarGroup.${g.id}`)),
          sidebarContainment.value[g.id].length > 0
            ? h('a',
                {
                  class: ['group-more-link', { 'is-active': route.name === `${g.id}-more` }],
                  // 原生 tooltip 说明该组收纳了哪些功能，给「更多」一个可预期的去向
                  title: sidebarContainment.value[g.id]
                    .map((n) => viewLabel(n))
                    .join(currentLocale.value === 'en-US' ? ', ' : '、'),
                  onClick: () => { void router.push({ name: `${g.id}-more` }) },
                },
                [
                  h(NIcon, { size: 14, class: 'group-more-icon' }, { default: () => h(LayersOutline) }),
                  t('common.nav.more'),
                  h(NIcon, { size: 12, class: 'group-more-caret' }, { default: () => h(ChevronForwardOutline) }),
                ],
              )
            : null,
        ]),
      children: g.views.map((name) => item(name)),
    })),
    item(PENULTIMATE_VIEW),
    item(LAST_VIEW),
  ]
})

/** 菜单级 nodeProps：仅当前在册成员附右键事件（issue #475 改用 isSidebarMember：
 *  主项与移回的种子可排序/移入；固定项与仍在清单的收纳成员无任何右键菜单）。
 *  naive-ui 的 nodeProps 返回类型把索引签名限成 string|number，事件函数过不去
 *  （类型仅对 data-* 友好），运行时照常铺到节点上，故此处断言放宽。 */
function nodeProps(option: MenuOption) {
  const name = option.key as string
  if (!isSidebarMember(name)) return {}
  return {
    onContextmenu: (e: MouseEvent) => showSortMenu(e, name),
  } as unknown as HTMLAttributes & Record<string, string | number | undefined>
}

// ---------------------------------------------------------------------------
// 侧栏右键排序菜单（issue #270，#359 收窄为组内；#474 增「移入更多」）：主项右键弹出
// 组内排序菜单（上移/下移/移顶/移底/移入更多/恢复默认），手动定位弹出，与行级右键菜单
// 同一模式；点选即重排或移入并立即持久化，菜单打开期间视图快捷键由既有弹层抑制机制压制。
// ---------------------------------------------------------------------------

const sortMenuShow = ref(false)
const sortMenuX = ref(0)
const sortMenuY = ref(0)
const sortTarget = ref<ContainableViewName | null>(null)

const sortMenuOptions = computed(() => {
  const target = sortTarget.value
  const gid = target ? groupOfView(target) : null
  if (!target || !gid) return []
  return buildSidebarSortMenuOptions(target, sidebarGroupOrders.value[gid])
})

/** 右键在册成员弹出排序菜单：先收起再 nextTick 展开，保证连续弹出时位置刷新。 */
function showSortMenu(e: MouseEvent, name: ContainableViewName) {
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
  // 「移入更多」（issue #474）：主项退出组内序、追加本组收纳清单尾，点选即持久化
  if (key === 'intoMore') {
    applyMoveIntoMore(target)
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

// 侧栏标题行（issue #566）：应用显示名 + 金额隐私模式眼睛按钮——标题文本消费 i18n
// 应用名键 common.app.name（ADR-0076：显示名不得在 i18n 之外硬编码，t() 随语言切换
// 重渲染）；眼睛按钮入口唯一（spec #564：不进设置页、无快捷键、无第二渲染点），
// 消费应用设置 store 同一状态；睁/闭两态图标与
// tooltip、aria-label 反映当前状态（文案经 i18n 双语，aria-pressed 携带开关态），
// 点击即切换并持久化；格式化层（@/utils/money）消费同一 ref，全应用金额即时掩码/恢复。
// 渲染函数读取响应式状态，语言/开关变化时随重新渲染；侧栏折叠（宽度归零）时按钮
// 不可见，展开即可切换（接受取舍，不设第二渲染点）。
const title = () => {
  // 无障碍标签/tooltip 反映当前状态（文案随界面语言）：关→「隐藏金额」、开→「显示金额」，
  // aria-pressed 携带开关态（WAI-ARIA toggle button 模式）
  const privacyLabel = store.amountPrivacyEnabled ? t('common.amountPrivacy.show') : t('common.amountPrivacy.hide')
  return h(
    'div',
    { style: 'display:flex;align-items:center;justify-content:space-between;gap:4px;min-width:0;padding:12px 8px 12px 16px;font-size:18px;font-weight:600' },
    [
      h('span', `📒 ${t('common.app.name')}`),
      h(
        NButton,
        {
          size: 'tiny',
          quaternary: true,
          circle: true,
          'aria-pressed': store.amountPrivacyEnabled,
          title: privacyLabel,
          'aria-label': privacyLabel,
          onClick: () => store.setAmountPrivacyEnabled(!store.amountPrivacyEnabled),
        },
        {
          icon: () =>
            h(NIcon, { size: 16 }, { default: () => h(store.amountPrivacyEnabled ? EyeOffOutline : EyeOutline) }),
        },
      ),
    ],
  )
}

const pageTitle = computed(() => (typeof route.name === 'string' ? viewLabel(route.name) : ''))
</script>

<template>
  <NConfigProvider
    :theme="store.theme === 'dark' ? darkTheme : null"
    :theme-overrides="store.theme === 'dark' ? darkOverrides : lightOverrides"
    :locale="naiveLocale"
    :date-locale="naiveDateLocale"
  >
    <!-- 全局忙碌条（issue #500）：非模态环境指示，只随忙碌状态渲染，
         不注册 Overlay Suppression（ADR-0035 豁免，见词汇表词条） -->
    <GlobalBusyBar />
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
/* 组标题行「更多」链接（issue #472 / ADR-0063 决策 1）。
   这些类位于 label 渲染函数 h() 创建的元素上，不带本组件 data-v 属性，
   scoped 原生类名选择器匹配不到，必须 :deep() 以 [data-v] 后代选择器命中
   （锚点为 NMenu 根元素，继承父组件 scopeId）。
   三态：静默与组标题同色 → hover 淡背景 + 实色 →
   所在组「更多」页激活时主色 + 选中淡背景（与子菜单项选中态一致）。 */
:deep(.sidebar-group-title) {
  display: flex;
  flex: 1;
  min-width: 0;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  /* 14px + 链接自身 6px 右 padding = 20px，链接盒（文字+箭头）右缘
     与上下菜单项键位提示对齐（.n-menu-item-content 18px + 键位行 2px）。 */
  padding-right: 14px;
}

:deep(.group-more-link) {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: 12px;
  color: var(--n-group-text-color);
  /* 扩大点击热区（约 24x16），不显眼地包住文字与箭头 */
  padding: 2px 6px;
  border-radius: var(--n-border-radius, 4px);
  cursor: pointer;
  text-decoration: none;
  transition:
    color .2s var(--n-bezier),
    background-color .2s var(--n-bezier);
}

:deep(.group-more-link:hover) {
  color: var(--n-item-text-color-hover);
  background-color: var(--n-item-color-hover);
}

/* 激活态置于 hover 之后：悬停已激活链接时保持选中背景不闪变 */
:deep(.group-more-link.is-active),
:deep(.group-more-link.is-active:hover) {
  color: var(--n-item-text-color-active);
  background-color: var(--n-item-color-active);
}
</style>
