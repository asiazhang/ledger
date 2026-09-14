import { computed, onScopeDispose, ref } from 'vue'
import type { ComputedRef } from 'vue'

/**
 * 窗口分级（Window Tier，ADR-0088 决策 2 / 词汇表「窗口分级」）：宽度轴唯一事实源。
 * 单一断点两档——≥ 断点为桌面档（既有布局），< 断点为移动档；本 composable 只做
 * 「宽度信号 → 档位」纯映射，不持任何按档位的渲染决策（形态断言归组件层测试）。
 */

/**
 * 窗口分级断点（px，全仓唯一出现点）：≥ 断点为桌面档，< 断点为移动档。
 * JS 侧一律消费本常量；CSS 媒体查询经构建期替换消费同值——CSS 源码书写
 * `__WINDOW_TIER_BREAKPOINT_PX__` 占位符，由 vite.config.ts 替换为本值，
 * 杜绝第二处魔法数字。
 */
export const WINDOW_TIER_BREAKPOINT_PX = 840

/** 桌面档媒体查询：由唯一断点常量派生（≥ 断点命中），移动档即其否定。 */
const DESKTOP_TIER_QUERY = `(min-width: ${WINDOW_TIER_BREAKPOINT_PX}px)`

/** 窗口档位闭集：桌面档 / 移动档。 */
export type WindowTier = 'desktop' | 'mobile'

/**
 * 窗口分级 composable：视口宽度媒体查询 → 档位。
 * 监听查询变化（窗口缩放实时换档），作用域销毁时注销监听；
 * 断点数值与查询构造全仓唯一收口于本文件。
 */
export function useWindowTier(): ComputedRef<WindowTier> {
  const mql = window.matchMedia(DESKTOP_TIER_QUERY)
  const isDesktop = ref(mql.matches)
  const onChange = (event: MediaQueryListEvent): void => {
    isDesktop.value = event.matches
  }
  mql.addEventListener('change', onChange)
  onScopeDispose(() => mql.removeEventListener('change', onChange))
  return computed<WindowTier>(() => (isDesktop.value ? 'desktop' : 'mobile'))
}
