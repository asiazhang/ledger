import { onMounted, onUnmounted } from 'vue'
import { isEditableTarget } from '@/composables/useCreateShortcuts'

/**
 * 窗口行为守卫（issue #154）：窗口层职责从应用层收回的唯一出处。
 * 在 document 捕获阶段统一注册两类拦截，应用根组件（App.vue）挂载一次：
 *
 * 1. **Escape 拦截（无条件）**：任何状态下按 ESC 都不作用于窗口层（macOS 上
 *    AppKit 默认把 ESC 交给 `cancelOperation:` 退出全屏，Web 层 preventDefault
 *    是唯一跨平台手段，且非全屏下 ESC 无系统行为，无条件拦截无副作用）。
 *    弹层关闭交给 naive-ui 默认行为（closeOnEsc），无弹层时无操作。
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
