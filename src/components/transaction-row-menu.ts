import { h, type Component } from 'vue'
import { NIcon, type DropdownOption } from 'naive-ui'
import { AddCircleOutline, CashOutline, TrashOutline } from '@vicons/ionicons5'
import type { Transaction } from '@/types'

/**
 * 行右键菜单图标渲染工厂（issue #177）：经 NIcon 包裹图标组件，尺寸随全局菜单样式统一。
 * 后续入口（编辑 CreateOutline 等）直接复用本工厂即可。
 *
 * 与 App.vue 内部同用途的 renderMenuIcon（按名查表 + 固定 18px）语义不同：
 * 行菜单靠全局菜单样式定尺寸，不传 size。命名加 Row 前缀以区分。
 */
export function renderRowMenuIcon(icon: Component): () => ReturnType<typeof h> {
  return () => h(NIcon, null, { default: () => h(icon) })
}

/**
 * 交易行右键菜单选项组装（issue #151 退款/删除 + issue #119 加入物品 + #177 图标化）：
 * 纯函数收口，菜单形状（项、顺序、禁用、图标、着色）可独立测试（#176 Testing Decisions）。
 *
 * 菜单形状：
 * - `expense` 行：退款（CashOutline）/ 加入物品（AddCircleOutline）/ 分隔线 / 删除（TrashOutline）；
 *   （issue #177 原文 CashBackOutline 在 @vicons/ionicons5 中不存在，改用语义最贴近的 CashOutline）
 * - 其余行（income | transfer | refund | buy | sell）：仅删除。
 *   「加入物品」仅对 expense 行呈现（溯源必为支出购买，ADR-0025）。
 *
 * `hasItem`：该交易已创建过物品（items store 按溯源指针比对得出，不新增查询）
 * → 「加入物品」置灰禁用（溯源唯一的界面呈现）。
 *
 * `errorColor`：当前主题的 error 色（组件经 useThemeVars 取值传入），注入删除项
 * DropdownOption props，图标+文字整体着色——不硬编码色值，暗色模式自动适配。
 */
export function buildRowMenuOptions(
  row: Pick<Transaction, 'kind'>,
  opts: { hasItem?: boolean; errorColor?: string } = {},
): DropdownOption[] {
  const options: DropdownOption[] = []
  if (row.kind === 'expense') {
    options.push({ label: '退款', key: 'refund', icon: renderRowMenuIcon(CashOutline) })
    options.push({
      label: '加入物品',
      key: 'add-item',
      disabled: opts.hasItem === true,
      icon: renderRowMenuIcon(AddCircleOutline),
    })
  }
  if (options.length > 0) {
    options.push({ type: 'divider', key: 'menu-divider' })
  }
  // 删除项着色（props 合并到 n-dropdown-option-body 节点）：内联 color 覆盖文字色；
  // 图标容器 __prefix 有独立 color: var(--n-prefix-color) 规则（不吃继承），需一并覆盖；
  // hover/键盘 pending 态的 body 与 prefix 颜色规则同源变量，同样覆盖以保持整体 error 色。
  const errorProps =
    opts.errorColor !== undefined
      ? {
          props: {
            style: {
              color: opts.errorColor,
              '--n-prefix-color': opts.errorColor,
              '--n-option-text-color-hover': opts.errorColor,
              '--n-option-text-color-active': opts.errorColor,
            },
          },
        }
      : {}
  options.push({
    label: '删除',
    key: 'delete',
    icon: renderRowMenuIcon(TrashOutline),
    ...errorProps,
  })
  return options
}
