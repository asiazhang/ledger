/**
 * 复位回调注册表（spec #892 唯一新缝）：「当前视图的保留态复位回调」是显式声明的
 * 应用状态，声明式上报形态仿弹层注册表（ADR-0035）——持有保留态的视图在 setup 期
 * 注册、作用域销毁时自动撤销（跨断点换档卸载、导航离开均兜底撤销，不滞留注册态，
 * 导航抽屉先例同款）。注册表不持业务语义：只存「当前视图注册的复位回调」一个槽位，
 * 复位做什么由注册方（视图经其状态模块的既有复位出口）决定。
 *
 * 消费方唯一：窗口行为守卫（issue #154 的既有 ESC 拦截出处）——ESC 两级语义
 * （spec #892 / ADR-0094）的第二级：无弹层时调用当前视图注册的复位回调；无注册
 * 即无操作。有弹层时守卫不消费本注册表（弹层库默认关闭行为接管）。
 */

import { onScopeDispose } from 'vue'

/** 当前视图注册的复位回调（单槽：同一时刻至多一个视图挂载，无 KeepAlive）。 */
let currentReset: (() => void) | null = null

/**
 * 注册当前视图的复位回调。必须在组件 setup 作用域内调用（作用域销毁时自动撤销）；
 * 重复注册以最后一次为准。
 */
export function registerViewReset(reset: () => void): void {
  currentReset = reset
  onScopeDispose(() => {
    if (currentReset === reset) currentReset = null
  })
}

/** 消费当前视图的复位回调：有注册则执行并返回 true，无注册无操作返回 false。 */
export function fireViewReset(): boolean {
  if (currentReset === null) return false
  currentReset()
  return true
}

/** 测试专用：清空注册表（模拟组件整体卸载后的干净状态）。 */
export function clearViewResets(): void {
  currentReset = null
}
