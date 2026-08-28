import type { DropdownOption } from 'naive-ui'
import type { Transaction } from '@/types'

/**
 * 交易行右键菜单选项组装（issue #151 退款/删除 + issue #119 加入物品）：
 * 纯函数收口，菜单形状（项、顺序、禁用）可独立测试（#176 Testing Decisions）。
 *
 * 菜单形状：
 * - `expense` 行：退款 / 加入物品 / 分隔线 / 删除；
 * - 其余行（income | transfer | refund | buy | sell）：仅删除。
 *   「加入物品」仅对 expense 行呈现（溯源必为支出购买，ADR-0025）。
 *
 * `hasItem`：该交易已创建过物品（items store 按溯源指针比对得出，不新增查询）
 * → 「加入物品」置灰禁用（溯源唯一的界面呈现）。
 */
export function buildRowMenuOptions(
  row: Pick<Transaction, 'kind'>,
  opts: { hasItem?: boolean } = {},
): DropdownOption[] {
  const options: DropdownOption[] = []
  if (row.kind === 'expense') {
    options.push({ label: '退款', key: 'refund' })
    options.push({ label: '加入物品', key: 'add-item', disabled: opts.hasItem === true })
  }
  if (options.length > 0) {
    options.push({ type: 'divider', key: 'menu-divider' })
  }
  options.push({ label: '删除', key: 'delete' })
  return options
}
