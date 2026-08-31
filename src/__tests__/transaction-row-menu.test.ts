import { describe, expect, it } from 'vitest'
import { NIcon } from 'naive-ui'
import type { DropdownOption } from 'naive-ui'
import type { VNode } from 'vue'
import { AddCircleOutline, CashOutline, CreateOutline, TrashOutline } from '@vicons/ionicons5'
import { buildRowMenuOptions, renderRowMenuIcon } from '@/components/transaction-row-menu'

/** 渲染 DropdownOption.icon 工厂，取出其中的图标组件（用于断言挂了哪个图标）。 */
function iconComponentOf(option: DropdownOption): unknown {
  const render = option.icon
  if (!render) return undefined
  const iconVNode = render() as VNode
  expect(iconVNode.type).toBe(NIcon)
  const slot = iconVNode.children as { default: () => VNode }
  return slot.default().type
}

describe('renderRowMenuIcon（行菜单图标渲染工厂）', () => {
  it('返回渲染函数，经 NIcon 包裹指定图标组件（尺寸由全局菜单样式统一）', () => {
    const render = renderRowMenuIcon(CashOutline)
    expect(typeof render).toBe('function')
    expect(iconComponentOf({ icon: render })).toBe(CashOutline)
  })
})

describe('buildRowMenuOptions（行右键菜单选项）', () => {
  it('expense 行：编辑 / 退款 / 加入物品 / 分隔线 / 删除，加入物品默认可用', () => {
    const options = buildRowMenuOptions({ kind: 'expense' })
    expect(options.map((o) => 'key' in o && o.key)).toEqual([
      'edit',
      'refund',
      'add-item',
      'menu-divider',
      'delete',
    ])
    const addItem = options.find((o) => 'key' in o && o.key === 'add-item')
    expect(addItem).toMatchObject({ label: '加入物品', disabled: false })
  })

  it('expense 行已建物品：加入物品置灰禁用（溯源唯一的界面呈现）', () => {
    const options = buildRowMenuOptions({ kind: 'expense' }, { hasItem: true })
    const addItem = options.find((o) => 'key' in o && o.key === 'add-item')
    expect(addItem).toMatchObject({ disabled: true })
    // 其余项不受影响
    expect(options.map((o) => 'key' in o && o.key)).toEqual([
      'edit',
      'refund',
      'add-item',
      'menu-divider',
      'delete',
    ])
  })

  it.each(['income', 'transfer'] as const)(
    '%s 行：编辑 / 分隔线 / 删除（无退款/加入物品）',
    (kind) => {
      const options = buildRowMenuOptions({ kind })
      expect(options.map((o) => 'key' in o && o.key)).toEqual(['edit', 'menu-divider', 'delete'])
      expect(options[0]).toMatchObject({ label: '编辑' })
      expect(options[0].disabled).toBeFalsy()
    },
  )

  it('buy 行：编辑 / 分隔线 / 删除（投资表单编辑模式，issue #180）', () => {
    const options = buildRowMenuOptions({ kind: 'buy' })
    expect(options.map((o) => 'key' in o && o.key)).toEqual(['edit', 'menu-divider', 'delete'])
    expect(options[0]).toMatchObject({ label: '编辑' })
    expect(options[0].disabled).toBeFalsy()
  })

  it('sell 行：编辑 / 分隔线 / 删除（投资表单编辑模式，issue #180）', () => {
    const options = buildRowMenuOptions({ kind: 'sell' })
    expect(options.map((o) => 'key' in o && o.key)).toEqual(['edit', 'menu-divider', 'delete'])
  })

  it('refund 行：仅删除（编辑破坏关联语义，本期边界外）', () => {
    const options = buildRowMenuOptions({ kind: 'refund' })
    expect(options.map((o) => 'key' in o && o.key)).toEqual(['delete'])
  })

  it('expense 行挂图标：编辑 CreateOutline、退款 CashOutline、加入物品 AddCircleOutline、删除 TrashOutline', () => {
    const options = buildRowMenuOptions({ kind: 'expense' })
    const byKey = (key: string) => options.find((o) => 'key' in o && o.key === key)!
    expect(iconComponentOf(byKey('edit'))).toBe(CreateOutline)
    expect(iconComponentOf(byKey('refund'))).toBe(CashOutline)
    expect(iconComponentOf(byKey('add-item'))).toBe(AddCircleOutline)
    expect(iconComponentOf(byKey('delete'))).toBe(TrashOutline)
    expect(byKey('menu-divider').icon).toBeUndefined()
  })

  it('删除项着主题 error 色：经 DropdownOption props 注入（不硬编码色值），图标+文字整体生效', () => {
    const errorColor = '#d03050' // 传入值即主题 errorColor（组件经 useThemeVars 取当前主题值）
    const options = buildRowMenuOptions({ kind: 'expense' }, { errorColor })
    const byKey = (key: string) => options.find((o) => 'key' in o && o.key === key)!
    const del = byKey('delete')
    // 文字色 + 图标前缀色（--n-prefix-color）+ hover/active 态，整体保持 error 色
    expect(del.props).toMatchObject({
      style: {
        color: errorColor,
        '--n-prefix-color': errorColor,
        '--n-option-text-color-hover': errorColor,
        '--n-option-text-color-active': errorColor,
      },
    })
    // 其余项不着色
    expect(byKey('edit').props).toBeUndefined()
    expect(byKey('refund').props).toBeUndefined()
    expect(byKey('add-item').props).toBeUndefined()
    // 非 expense 行的删除项同样着色
    const only = buildRowMenuOptions({ kind: 'income' }, { errorColor })
    expect(only[only.length - 1].props).toMatchObject({
      style: { color: errorColor, '--n-prefix-color': errorColor },
    })
  })

  it('未传 errorColor 时删除项不注入 props（向后兼容）', () => {
    const options = buildRowMenuOptions({ kind: 'expense' })
    const del = options.find((o) => 'key' in o && o.key === 'delete')!
    expect(del.props).toBeUndefined()
  })
})
