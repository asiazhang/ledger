import { onMounted, onUnmounted } from 'vue'
import type { Router } from 'vue-router'

export interface ViewShortcut {
  /** 路由 name（与侧边栏菜单 key 一致） */
  name: string
  /** 主键：数字 '1'..'0' 或 ','（设置） */
  key: string
}

/**
 * 侧边栏视图单一来源（顺序 = 菜单顺序 = 数字键位）。
 * 排序原则：日常记账动线 → 资金规划 → 分析工具 → 低频/系统。
 * 每个视图恰好一个快捷键：数字 1–0 按菜单位置对应前 10 个视图，
 * 设置用 Cmd/Ctrl+,（macOS「设置」惯例键位，避免占用 Cmd+S 的「保存」肌肉记忆）。
 */
export const sidebarViews: Array<{ name: string; key?: string }> = [
  { name: 'dashboard', key: '1' },
  { name: 'transactions', key: '2' },
  { name: 'accounts', key: '3' },
  { name: 'budget', key: '4' },
  // 订阅（issue #159）：原无快捷键（1..9 已占满），重排后归入数字位 5
  { name: 'subscriptions', key: '5' },
  { name: 'investments', key: '6' },
  { name: 'reports', key: '7' },
  { name: 'search', key: '8' },
  // 物品（issue #116）：重排后归入数字位 9
  { name: 'items', key: '9' },
  { name: 'ai', key: '0' },
  { name: 'settings', key: ',' },
]

/** 视图快捷键映射：按侧边栏菜单顺序，Cmd/Ctrl+1..0 与 Cmd/Ctrl+,。 */
export const viewShortcuts: ViewShortcut[] = sidebarViews
  .filter((v): v is { name: string; key: string } => v.key !== undefined)
  .map(({ name, key }) => ({ name, key }))

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
  return viewShortcuts.find((s) => e.key === s.key)?.name ?? null
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
