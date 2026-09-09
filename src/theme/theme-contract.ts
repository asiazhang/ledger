import type { GlobalThemeOverrides } from 'naive-ui'
import type { Theme } from '@/stores/app'
import { darkOverrides, lightOverrides } from './overrides'
import { darkThemeClass, lightThemeClass } from './app-theme.css.ts'

/**
 * 主题合同（issue #888 / ADR-0093）：输入亮/暗模式（Appearance 设备偏好的唯一
 * 状态源 `useAppStore().theme`），产出应用根元素主题类与同模式的组件库主题覆盖。
 * App.vue 是唯一接线点——根主题类经 `bindRootThemeClass` 绑定 document.body
 * （新方案 vanilla-extract 样式的变量宿主），组件库覆盖经 NConfigProvider
 * `theme-overrides` 注入；两条产物同源自同一模式，并行不互扰。
 */

/** 主题合同产物：根元素主题类 + 组件库主题覆盖（同一模式的一次解析）。 */
export interface AppThemeContract {
  /** 绑定应用根元素的 vanilla-extract 主题类（新方案样式的变量宿主） */
  rootClass: string
  /** 组件库主题覆盖（与 rootClass 同模式，从 token 派生） */
  overrides: GlobalThemeOverrides
}

/** 按模式解析主题合同（纯函数：同一模式恒定产出，无第二主题状态源）。 */
export function resolveAppTheme(theme: Theme): AppThemeContract {
  return theme === 'dark'
    ? { rootClass: darkThemeClass, overrides: darkOverrides }
    : { rootClass: lightThemeClass, overrides: lightOverrides }
}

/** 把主题类绑定到应用根元素（document.body）：挂当前模式类、移除另一模式类——
 * 换装是单一动作；teleport 到 body 的弹层同域继承主题变量。 */
export function bindRootThemeClass(rootClass: string): void {
  for (const cls of [darkThemeClass, lightThemeClass]) {
    document.body.classList.toggle(cls, cls === rootClass)
  }
}
