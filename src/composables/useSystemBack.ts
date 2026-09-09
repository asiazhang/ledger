import { onScopeDispose, watch } from 'vue'
import { useRouter } from 'vue-router'
import { onBackButtonPress } from '@tauri-apps/api/app'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { closeTopOverlay, hasOpenOverlay } from '@/composables/overlayRegistry'
import { useWindowTier } from '@/composables/useWindowTier'

/**
 * 系统返回桥接（issue #845 / ADR-0088 决策 7，词汇表「系统返回语义」）：
 * Android 返回键 / 返回手势获得应用内语义，应用根组件挂载一次。
 *
 * **到达方式（Tauri v2 官方接线点，纯前端、零原生代码）**：tauri 2.11 Android
 * 层内置 AppPlugin 的 OnBackPressedCallback——JS 侧经 `onBackButtonPress`
 * （@tauri-apps/api/app）注册监听后，返回键不再走「WebView goBack / Activity
 * finish」默认分支，而是携带 `canGoBack` 载荷派发本事件；权限
 * `core:app:allow-register-listener` 已含于 core:default，零权限改动。
 *
 * **挂载门禁**：按窗口分级——仅移动档注册监听，桌面档不挂此通道（无返回键即无
 * 此语义，两档互不渗透，ADR-0088）；跨断点换档与作用域销毁时撤销注册，换档与
 * 注册完成之间的竞态以代际号判定，迟到注册自撤。
 *
 * **语义三段**：
 * 1. 有弹层 → 关最上层（复用弹层注册表判定与关闭出口 closeTopOverlay，与 ESC
 *    同构只关最上层；遮罩不关原则不动——返回键是显式动作第四通道）；
 * 2. 无弹层且可回退 → 路由回退（WebView history 后退，hash 导航由 vue-router
 *    接管；返回 ≠ 清空，不触发视图复位通道，ADR-0094）；
 * 3. 栈底（canGoBack=false）→ 交还系统：销毁主窗口结束 Activity——监听注册后
 *    原生默认分支已旁路，此调用等价其 finish 分支（Android 专属 capability
 *    `core:window:allow-destroy`，桌面构建不含此权限）；真机行为归票⑩冒烟。
 *
 * 栈顶弹层不可关（退化用法，无关闭通道）时吞掉本次返回键：不回退路由（弹层
 * 开着时路由回退会把状态埋进弹层下，属数据丢失面）、不交还系统。
 */
export function useSystemBack(): void {
  const tier = useWindowTier()
  const router = useRouter()

  // 当前档位的注册撤销句柄（注册是异步的，完成前换档由代际判定自撤）
  let activeUnlisten: (() => void) | null = null
  let generation = 0

  function handleBackPress(payload: { canGoBack: boolean }): void {
    // 有弹层：本次返回键由弹层层消费——关最上层；栈顶不可关的退化用法则吞掉，
    // 不把返回透给路由层（弹层开着时路由回退会把状态埋进弹层下）
    if (hasOpenOverlay()) {
      closeTopOverlay()
      return
    }
    if (payload.canGoBack) {
      router.back()
      return
    }
    // 栈底交还系统；非 Tauri 环境（测试/浏览器）拒绝时静默（先例 App.vue 标题）
    getCurrentWindow().destroy().catch(() => {})
  }

  watch(
    tier,
    (value) => {
      generation += 1
      activeUnlisten?.()
      activeUnlisten = null
      if (value !== 'mobile') return
      const gen = generation
      onBackButtonPress(handleBackPress)
        .then((listener) => {
          if (gen !== generation) {
            // 注册完成前已换档/卸载：撤销迟到注册
            void listener.unregister()
            return
          }
          activeUnlisten = () => void listener.unregister()
        })
        .catch(() => {
          // 注册失败：返回通道惰性禁用（原生默认分支接管），不崩溃
        })
    },
    { immediate: true },
  )

  onScopeDispose(() => {
    generation += 1
    activeUnlisten?.()
    activeUnlisten = null
  })
}
