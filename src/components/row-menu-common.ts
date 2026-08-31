import { h, type Component } from 'vue'
import { NIcon } from 'naive-ui'

/**
 * 行内操作菜单公共件：交易行菜单（transaction-row-menu）与账户行菜单
 * （account-row-menu）共享的图标渲染与删除项着色逻辑。
 */

/**
 * 行菜单图标渲染工厂：经 NIcon 包裹图标组件，尺寸随全局菜单样式统一。
 *
 * 与 App.vue 内部同用途的 renderMenuIcon（按名查表 + 固定 18px）语义不同：
 * 行菜单靠全局菜单样式定尺寸，不传 size。命名加 Row 前缀以区分。
 */
export function renderRowMenuIcon(icon: Component): () => ReturnType<typeof h> {
  return () => h(NIcon, null, { default: () => h(icon) })
}

/**
 * 删除项着主题 error 色：经 DropdownOption props 注入（不硬编码色值，暗色模式
 * 自动适配）——内联 color 覆盖文字色；图标容器 __prefix 有独立
 * `color: var(--n-prefix-color)` 规则（不吃继承），需一并覆盖；hover/键盘 pending
 * 态的 body 与 prefix 颜色规则同源变量，同样覆盖以保持整体 error 色。
 * 未传 errorColor 时返回空对象（不注入 props，向后兼容）。
 */
export function errorOptionProps(errorColor?: string): Record<string, unknown> {
  return errorColor === undefined
    ? {}
    : {
        props: {
          style: {
            color: errorColor,
            '--n-prefix-color': errorColor,
            '--n-option-text-color-hover': errorColor,
            '--n-option-text-color-active': errorColor,
          },
        },
      }
}
