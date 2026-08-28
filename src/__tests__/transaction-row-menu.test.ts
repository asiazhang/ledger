import { describe, expect, it } from 'vitest'
import { buildRowMenuOptions } from '@/components/transaction-row-menu'

describe('buildRowMenuOptions（行右键菜单选项）', () => {
  it('expense 行：退款 / 加入物品 / 分隔线 / 删除，加入物品默认可用', () => {
    const options = buildRowMenuOptions({ kind: 'expense' })
    expect(options.map((o) => 'key' in o && o.key)).toEqual([
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
      'refund',
      'add-item',
      'menu-divider',
      'delete',
    ])
  })

  it.each(['income', 'transfer', 'refund', 'buy', 'sell'] as const)(
    '%s 行：仅删除（无退款/加入物品）',
    (kind) => {
      const options = buildRowMenuOptions({ kind })
      expect(options.map((o) => 'key' in o && o.key)).toEqual(['delete'])
    },
  )
})
