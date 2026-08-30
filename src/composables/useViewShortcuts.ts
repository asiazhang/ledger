import { computed, onMounted, onUnmounted, ref } from 'vue'
import type { Router } from 'vue-router'
import { getSavedSidebarOrder } from '@/utils/view-state'

export interface ViewShortcut {
  /** 路由 name（与侧边栏菜单 key 一致） */
  name: string
  /** 主键：按最终位置推导的数字 '1'..'0' 或 ','（设置） */
  key: string
}

/**
 * 顺序源模块：侧边栏视图顺序单一来源（顺序 = 菜单顺序 = 数字键位）。
 * 默认序按使用频率物化为出厂快照（不做运行时统计）；
 * 三固定约束与可排区边界编码为常量，键位与侧栏菜单均由最终序按位置推导。
 */

/** 默认序（按使用频率的出厂快照）：概览、交易、账户、预算、投资、报表、定时、物品、搜索、AI、设置 */
export const DEFAULT_VIEW_ORDER = [
  'dashboard',
  'transactions',
  'accounts',
  'budget',
  'investments',
  'reports',
  'scheduled',
  'items',
  'search',
  'ai',
  'settings',
] as const

export type ViewName = (typeof DEFAULT_VIEW_ORDER)[number]

/** 三固定约束：概览首位（与启动落地页一致）、AI 倒数第二、设置末位 */
export const FIRST_VIEW: ViewName = 'dashboard'
export const PENULTIMATE_VIEW: ViewName = 'ai'
export const LAST_VIEW: ViewName = 'settings'

/** 可排区（第 2–9 位）：默认序去掉三固定项，相对顺序即默认相对顺序 */
export const ARRANGEABLE_VIEWS: readonly ViewName[] = DEFAULT_VIEW_ORDER.filter(
  (name) => name !== FIRST_VIEW && name !== PENULTIMATE_VIEW && name !== LAST_VIEW,
)

function isArrangeable(v: unknown): v is ViewName {
  return typeof v === 'string' && (ARRANGEABLE_VIEWS as readonly string[]).includes(v)
}

/**
 * 顺序解析（纯函数）：已存可排区顺序（脏数据防御）→ 最终可排区顺序。
 * 非法视图名过滤（含三固定项名、未知名、非字符串项）→ 去重（保留首现）→
 * 缺失项按默认序补入可排区末尾；非数组输入整体回退默认序。
 */
export function parseArrangeableOrder(raw: unknown): ViewName[] {
  const kept: ViewName[] = []
  const seen = new Set<string>()
  if (Array.isArray(raw)) {
    for (const item of raw) {
      if (!isArrangeable(item) || seen.has(item)) continue
      seen.add(item)
      kept.push(item)
    }
  }
  for (const name of ARRANGEABLE_VIEWS) {
    if (!seen.has(name)) kept.push(name)
  }
  return kept
}

/**
 * 已存可排区顺序：启动读路径（issue #269 仅有读路径，读取恒为空 → 回退默认序；
 * 写路径与右键自定义排序由后续票落地，届时只需更新此 ref）。
 */
const arrangeableOrder = ref<ViewName[]>(parseArrangeableOrder(getSavedSidebarOrder()))

/** 键位按最终位置推导：第 1–9 位 → '1'..'9'，第 10 位 → '0'，第 11 位（设置）→ ',' */
const POSITION_KEYS = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0', ','] as const

/**
 * 视图快捷键映射（响应式）：由最终序按位置推导，键随位置（重排侧栏即重排键位）。
 * 每个视图恰好一个快捷键：数字 1–0 对应第 1–10 位，设置用 Cmd/Ctrl+,（macOS「设置」
 * 惯例键位，避免占用 Cmd+S 的「保存」肌肉记忆）。
 */
export const viewShortcuts = computed<ViewShortcut[]>(() => {
  const order: ViewName[] = [FIRST_VIEW, ...arrangeableOrder.value, PENULTIMATE_VIEW, LAST_VIEW]
  return order.map((name, i) => ({ name, key: POSITION_KEYS[i] }))
})

/** macOS 用 Cmd（metaKey），Windows/Linux 用 Ctrl（ctrlKey） */
export function isMacPlatform(): boolean {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } }
  const platform = nav.userAgentData?.platform ?? navigator.platform
  return /mac/i.test(platform)
}

/** 菜单提示文案：macOS 显示 ⌘1，其余显示 Ctrl+1 */
export function shortcutHint(key: string): string {
  return isMacPlatform() ? `⌘${key}` : `Ctrl+${key}`
}

/** 是否恰按主修饰键（不混按 Ctrl/Cmd 双键） */
function isPrimaryModifier(e: KeyboardEvent): boolean {
  return isMacPlatform() ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey
}

/** 纯函数：命中视图快捷键则返回路由 name，否则 null（排除 Shift/Alt/混按/未映射键） */
export function matchViewShortcut(e: KeyboardEvent): string | null {
  if (e.altKey || e.shiftKey) return null
  if (!isPrimaryModifier(e)) return null
  return viewShortcuts.value.find((s) => e.key === s.key)?.name ?? null
}

/** 弹层探测选择器：Naive UI 弹层「仅打开时存在」的元素（issue #153 扩展弹层类）。
 *
 * 模态类（弹窗/useDialog 确认框）一律探测 `.n-modal-mask` 遮罩：遮罩随 show 真实挂载/卸载。
 * 不能探测 `.n-modal-container` / `.n-dialog`——naive-ui 的 VLazyTeleport 采用
 * useFalseUntilTruthy 语义，容器首次显示后永久残留 DOM（关闭后只剩隐藏空壳），
 * 存在性嗅探会把「已关闭」误判为「打开」，导致快捷键永久失效（见 ADR-0021）。
 * `.n-date-panel` 为无遮罩弹层（日期日历），内部按钮聚焦时不在可编辑目标，需单独纳入信号集。
 */
const OVERLAY_SELECTORS = [
  '.n-modal-mask',
  '.n-popconfirm',
  '.n-dropdown-menu',
  '.n-base-select-menu',
  '.n-date-panel',
]

/** 覆盖层（弹窗/确认框/下拉菜单）打开时抑制快捷键，避免在编辑/确认/选择中途触发 */
export function hasOpenOverlay(): boolean {
  return OVERLAY_SELECTORS.some((sel) => document.querySelector(sel) !== null)
}

/** 注册全局 keydown 监听：命中视图快捷键时切换路由 */
export function useViewShortcuts(router: Router) {
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
}
