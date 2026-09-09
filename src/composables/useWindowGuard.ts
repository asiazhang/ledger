import { onMounted, onUnmounted } from 'vue'
import { isEditableTarget } from '@/composables/useCreateShortcuts'
import { hasOpenOverlay } from '@/composables/overlayRegistry'
import { fireViewReset } from '@/composables/viewResetRegistry'

/**
 * 窗口行为守卫（issue #154）：窗口层职责从应用层收回的唯一出处。
 * 在 document 捕获阶段统一注册两类拦截，应用根组件（App.vue）挂载一次：
 *
 * 1. **Escape 拦截（无条件）**：任何状态下按 ESC 都不作用于窗口层（macOS 上
 *    AppKit 默认把 ESC 交给 `cancelOperation:` 退出全屏，Web 层 preventDefault
 *    是唯一跨平台手段，且非全屏下 ESC 无系统行为，无条件拦截无副作用）。
 *    ESC 两级语义（spec #892 / ADR-0094）：有弹层时弹层关闭交给 naive-ui 默认
 *    行为（closeOnEsc），本守卫不叠加动作；无弹层时调用当前视图经复位回调注册表
 *    （viewResetRegistry，spec #892 唯一新缝）注册的复位回调，无注册即无操作。
 *    弹层判定复用弹层注册表（ADR-0035），不做 DOM 推断。
 * 2. **原生右键菜单拦截（带例外）**：默认 preventDefault 禁用 WKWebView 自带的
 *    Back/Reload 菜单；目标为可编辑元素（input/textarea/contenteditable，判定
 *    复用记一笔快捷键的 isEditableTarget）时放行，保留系统编辑菜单。
 *
 * 两类拦截**只 preventDefault，不 stopPropagation**：naive-ui 弹层（NModal
 * closeOnEsc）与行级自定义右键菜单（NDropdown 读取事件坐标）都依赖事件继续传播。
 */
export function useWindowGuard() {
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key !== 'Escape') return
    e.preventDefault()
    // 两级语义（spec #892）：有弹层 → 弹层库默认行为关最上层弹层，不叠加动作；
    // 无弹层 → 消费当前视图注册的复位回调（无注册即无操作；无保留状态的视图
    // 不注册，天然无操作）。只 preventDefault 不阻断传播：弹层库的 ESC 关闭
    // 依赖事件继续传播，复位回调在拦截之后同步执行，不抢弹层行为。
    if (hasOpenOverlay()) return
    fireViewReset()
  }
  const onContextMenu = (e: MouseEvent) => {
    if (isEditableTarget(e)) return
    e.preventDefault()
  }
  onMounted(() => {
    document.addEventListener('keydown', onKeyDown, true)
    document.addEventListener('contextmenu', onContextMenu, true)
  })
  onUnmounted(() => {
    document.removeEventListener('keydown', onKeyDown, true)
    document.removeEventListener('contextmenu', onContextMenu, true)
  })
}
