import { describe, expect, it } from 'vitest'
import { NIcon } from 'naive-ui'
import type { DropdownOption } from 'naive-ui'
import type { VNode } from 'vue'
import { CreateOutline, SwapHorizontalOutline, TrashOutline } from '@vicons/ionicons5'
import { buildAccountRowMenuOptions } from '@/components/account-row-menu'
import { renderRowMenuIcon } from '@/components/transaction-row-menu'

/** 渲染 DropdownOption.icon 工厂，取出其中的图标组件（用于断言挂了哪个图标）。 */
function iconComponentOf(option: DropdownOption): unknown {
  const render = option.icon
  if (!render) return undefined
  const iconVNode = render() as VNode
  expect(iconVNode.type).toBe(NIcon)
  const slot = iconVNode.children as { default: () => VNode }
  return slot.default().type
}

describe('buildAccountRowMenuOptions（账户行菜单选项）', () => {
  it('菜单形状：编辑 / 调整余额 / 分隔线 / 删除（所有账户行一致）', () => {
    const options = buildAccountRowMenuOptions()
    expect(options.map((o) => 'key' in o && o.key)).toEqual([
      'edit',
      'adjust-balance',
      'menu-divider',
      'delete',
    ])
    const byKey = (key: string) => options.find((o) => 'key' in o && o.key === key)!
    expect(byKey('edit').label).toBe('编辑')
    expect(byKey('adjust-balance').label).toBe('调整余额')
    expect(byKey('delete').label).toBe('删除')
  })

  it('图标：编辑 CreateOutline、调整余额 SwapHorizontalOutline（调整即与黑洞账户的转账）、删除 TrashOutline', () => {
    const options = buildAccountRowMenuOptions()
    const byKey = (key: string) => options.find((o) => 'key' in o && o.key === key)!
    expect(iconComponentOf(byKey('edit'))).toBe(CreateOutline)
    expect(iconComponentOf(byKey('adjust-balance'))).toBe(SwapHorizontalOutline)
    expect(iconComponentOf(byKey('delete'))).toBe(TrashOutline)
    expect(byKey('menu-divider').icon).toBeUndefined()
  })

  it('删除项着主题 error 色：经 DropdownOption props 注入，其余项不着色', () => {
    const errorColor = '#d03050'
    const options = buildAccountRowMenuOptions({ errorColor })
    const byKey = (key: string) => options.find((o) => 'key' in o && o.key === key)!
    expect(byKey('delete').props).toMatchObject({
      style: {
        color: errorColor,
        '--n-prefix-color': errorColor,
        '--n-option-text-color-hover': errorColor,
        '--n-option-text-color-active': errorColor,
      },
    })
    expect(byKey('edit').props).toBeUndefined()
    expect(byKey('adjust-balance').props).toBeUndefined()
  })

  it('未传 errorColor 时删除项不注入 props', () => {
    const options = buildAccountRowMenuOptions()
    const del = options.find((o) => 'key' in o && o.key === 'delete')!
    expect(del.props).toBeUndefined()
  })

  it('图标渲染工厂与交易行菜单共用同一公共件（renderRowMenuIcon 来自 row-menu-common）', () => {
    expect(typeof renderRowMenuIcon(CreateOutline)).toBe('function')
  })
})
