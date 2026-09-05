import { computed, onMounted, onUnmounted } from 'vue'
import type { Router } from 'vue-router'
import { hasOpenOverlay } from '@/composables/overlayRegistry'
import { useSidebarOrderStore, FIRST_VIEW, PENULTIMATE_VIEW, LAST_VIEW, SIDEBAR_GROUPS } from '@/stores/sidebar-order'
import type { SidebarGroupId, SidebarGroupOrders } from '@/stores/sidebar-order'

export interface ViewShortcut {
  /** 路由 name（与侧边栏菜单 key 一致） */
  name: string
  /** 主键：概览 '`'、各组固定键位带（'1'..'9'）、'0'（AI）或 ','（设置）；null = 无键位 */
  key: string | null
}

/**
 * 视图快捷键模块（键位带段）：只管键位带推导与键盘注册，对 sidebar-order store
 * 只读消费组内序（issue #524/#549：顺序状态归位 store，本模块不再持任何顺序状态）。
 * 前提：须在 pinia 安装后使用（应用入口先装 pinia 再挂路由；测试经 setActivePinia）——
 * 组内序现读自 store，无 pinia 实例时无状态可读。
 */

// ---------------------------------------------------------------------------
// 键位按固定组带推导（ADR-0065，取代 ADR-0063 决策 2 的线性连续取键）：
// 概览恒 '`'（运行时验证前提见 ADR-0065 决策 1）；三组按固定组序各占三键带
// ——记账 1–3、资产 4–6、洞察 7–9，组内序定带内位置；AI 恒 '0'；设置为唯一例外用 ','。
// 收纳成员与「更多」链接不在推导表内——无键位、不出提示、不可键盘触发。
// 组主项不足 3 时带内空键保留（出厂 ⌘6、⌘9 空置），不跨组压缩；
// 1–9 九键对 3×3 理论上限，键位封闭严格成立；组内重排键位带内随动。
// ---------------------------------------------------------------------------

const LEAD_KEY = '`'
/** 每组固定键位带（组序固定，带序随组序）：记账 1–3、资产 4–6、洞察 7–9（ADR-0065） */
const GROUP_KEY_BANDS: Record<SidebarGroupId, readonly string[]> = {
  bookkeeping: ['1', '2', '3'],
  assets: ['4', '5', '6'],
  insights: ['7', '8', '9'],
}
const PENULTIMATE_KEY = '0'
const LAST_KEY = ','

/**
 * 键位推导纯函数：由组内序按固定组带派生全部视图快捷键（组内序定带内位置，重排即带内重排键位）。
 * 只扫主项：组主项不足 3 时带尾空键保留（不产记录）；超位（理论上限内不发生）得 null。
 */
export function deriveViewShortcuts(orders: SidebarGroupOrders): ViewShortcut[] {
  const shortcuts: ViewShortcut[] = [{ name: FIRST_VIEW, key: LEAD_KEY }]
  for (const g of SIDEBAR_GROUPS) {
    const band = GROUP_KEY_BANDS[g.id]
    orders[g.id].forEach((name, i) => {
      shortcuts.push({ name, key: band[i] ?? null })
    })
  }
  shortcuts.push({ name: PENULTIMATE_VIEW, key: PENULTIMATE_KEY })
  shortcuts.push({ name: LAST_VIEW, key: LAST_KEY })
  return shortcuts
}

/** macOS 用 Cmd（metaKey），Windows/Linux 用 Ctrl（ctrlKey） */
export function isMacPlatform(): boolean {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } }
  const platform = nav.userAgentData?.platform ?? navigator.platform
  return /mac/i.test(platform)
}

/** 菜单提示文案：macOS 显示 ⌘1，其余显示 ⌃1（⌃ 为 Control 键符，与 ⌘ 同族单字形、跨平台等宽对齐） */
export function shortcutHint(key: string): string {
  return isMacPlatform() ? `⌘${key}` : `⌃${key}`
}

/** 是否恰按主修饰键（不混按 Ctrl/Cmd 双键） */
function isPrimaryModifier(e: KeyboardEvent): boolean {
  return isMacPlatform() ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey
}

/** 纯函数：命中视图快捷键则返回路由 name，否则 null（排除 Shift/Alt/混按/未映射键/无键位项）。
 *  组内序现读自 sidebar-order store（每次调用现读，无模块级缓存）。 */
export function matchViewShortcut(e: KeyboardEvent): string | null {
  if (e.altKey || e.shiftKey) return null
  if (!isPrimaryModifier(e)) return null
  const shortcuts = deriveViewShortcuts(useSidebarOrderStore().sidebarGroupOrders)
  return shortcuts.find((s) => s.key !== null && e.key === s.key)?.name ?? null
}

/** 注册全局 keydown 监听：命中视图快捷键时切换路由。
 *  返回的 viewShortcuts 是响应式键位表（组内序变更时带内重排键位），供菜单键位提示消费；
 *  绑定在调用方的 store 实例上（不用模块级单例 computed：持久化状态归 store，测试经
 *  setActivePinia 换实例装配）。 */
export function useViewShortcuts(router: Router) {
  const store = useSidebarOrderStore()
  const viewShortcuts = computed<ViewShortcut[]>(() => deriveViewShortcuts(store.sidebarGroupOrders))

  const onKeydown = (e: KeyboardEvent) => {
    const name = matchViewShortcut(e)
    if (!name) return
    if (hasOpenOverlay()) return
    e.preventDefault()
    if (router.currentRoute.value.name !== name) {
      router.push({ name })
    }
  }
  onMounted(() => window.addEventListener('keydown', onKeydown))
  onUnmounted(() => window.removeEventListener('keydown', onKeydown))
  return { viewShortcuts }
}
