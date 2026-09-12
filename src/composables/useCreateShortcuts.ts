import { onUnmounted, watch } from 'vue'
import { hasOpenOverlay } from '@/composables/overlayRegistry'
import { useInputMode } from '@/composables/useInputMode'
import type { CreateTransactionKind } from '@ledger/types'

/**
 * 「记一笔」裸键快捷键（issue #153）：交易页按 a/z/i/b/s 直达对应类型的记一笔弹窗。
 * 键位映射是单一来源：keydown 匹配与下拉菜单项标注共用，保证提示与行为一致。
 * refund 不占键位（退款入口由交易条目右键菜单承接）；convert 无手工录入入口
 * （无现金腿 kind 界面只读，ADR-0106 决策 10 / #1048），同样不占键位。
 */
export const CREATE_KIND_KEYS: Record<CreateTransactionKind, string> = {
  expense: 'a',
  transfer: 'z',
  income: 'i',
  buy: 'b',
  sell: 's',
}

/**
 * 纯函数：裸键命中则返回对应 CreateTransactionKind，否则 null。
 * 仅接受无修饰键的小写裸键——任何 Ctrl/Cmd/Alt/Shift 修饰、大写（含 CapsLock）
 * 均不命中，把组合键让给系统与其他快捷键（如 Cmd+1..9 视图切换）。
 */
export function matchCreateShortcut(e: KeyboardEvent): CreateTransactionKind | null {
  if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return null
  const found = (Object.entries(CREATE_KIND_KEYS) as [CreateTransactionKind, string][]).find(
    ([, key]) => e.key === key,
  )
  return found?.[0] ?? null
}

/**
 * 纯函数：事件焦点是否位于可编辑元素（input/textarea/select/contenteditable）。
 * 命中时抑制快捷键，避免用户在输入/筛选时误开弹窗；
 * 同时是窗口行为守卫的右键放行判定（issue #154，保留系统编辑菜单）。
 */
export function isEditableTarget(e: Event): boolean {
  const el = e.target
  if (!(el instanceof HTMLElement)) return false
  if (el.isContentEditable) return true
  // isContentEditable 兜底：jsdom 未实现该属性，按 contenteditable 属性判断
  //（'' 与 'true' 均为可编辑，'false' 为显式不可编辑）
  const ce = el.getAttribute('contenteditable')
  if (ce === '' || ce === 'true') return true
  return el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT'
}

/**
 * 交易页「记一笔」快捷键：裸键 a/z/i/b/s 打开对应类型弹窗（复用 #150 的弹窗入口）。
 * 仅在交易页挂载（随视图装卸），抑制条件：焦点在可编辑元素或任一弹层打开。
 *
 * 触控轴退役（ADR-0088 决策 6 / issue #843）：指针轴注册裸键监听；触控轴不绑定
 * （含运行中换轴实时拆装——输入轴信号变化即重接线）；卸载时清理。监听面由输入轴
 * composable 唯一事实源驱动，纯函数 matchCreateShortcut 与轴无关（可独立测试）。
 */
export function useCreateShortcuts(open: (kind: CreateTransactionKind) => void) {
  const onKeydown = (e: KeyboardEvent) => {
    const kind = matchCreateShortcut(e)
    if (!kind) return
    if (isEditableTarget(e) || hasOpenOverlay()) return
    e.preventDefault()
    open(kind)
  }
  const inputMode = useInputMode()
  watch(
    inputMode,
    (mode) => {
      if (mode === 'pointer') window.addEventListener('keydown', onKeydown)
      else window.removeEventListener('keydown', onKeydown)
    },
    { immediate: true },
  )
  onUnmounted(() => window.removeEventListener('keydown', onKeydown))
}
