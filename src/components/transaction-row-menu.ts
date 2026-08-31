import type { DropdownOption } from 'naive-ui'
import { AddCircleOutline, CashOutline, CreateOutline, TrashOutline } from '@vicons/ionicons5'
import { errorOptionProps, renderRowMenuIcon } from './row-menu-common'
import { t } from '@/i18n'
import type { Transaction } from '@/types'

// 公共件（row-menu-common）原生于本模块：renderRowMenuIcon / errorOptionProps
// 的完整注释见该文件，此处重导出保持既有 import 路径不变。
export { renderRowMenuIcon, errorOptionProps }

/**
 * 交易行右键菜单选项组装（issue #151 退款/删除 + issue #119 加入物品 + #177 图标化）：
 * 纯函数收口，菜单形状（项、顺序、禁用、图标、着色）可独立测试（#176 Testing Decisions）。
 *
 * 菜单形状（issue #178 增编辑项，issue #180 扩到 buy/sell）：
 * - `income | expense | transfer | buy | sell` 行：编辑（CreateOutline）在最前；
 * - `expense` 行另有：退款（CashOutline）/ 加入物品（AddCircleOutline）；
 *   （issue #177 原文 CashBackOutline 在 @vicons/ionicons5 中不存在，改用语义最贴近的 CashOutline）
 * - `refund` 行：仅删除。
 *   「编辑」对除 refund 外的 kind 呈现（refund 破坏关联语义；buy/sell 经投资表单
 *   编辑模式回填标的/数量/价格/费用，issue #180）；
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
  // 「编辑」显式白名单（refund 破坏关联语义不开放；其余 kind 经各自表单编辑：
  // income/expense/transfer 走分类记账/转账表单，buy/sell 走投资表单，issue #180）。
  if (row.kind === 'income' || row.kind === 'expense' || row.kind === 'transfer'
    || row.kind === 'buy' || row.kind === 'sell') {
    options.push({ label: t('transactions.menu.edit'), key: 'edit', icon: renderRowMenuIcon(CreateOutline) })
  }
  if (row.kind === 'expense') {
    options.push({ label: t('transactions.menu.refund'), key: 'refund', icon: renderRowMenuIcon(CashOutline) })
    options.push({
      label: t('transactions.menu.addItem'),
      key: 'add-item',
      disabled: opts.hasItem === true,
      icon: renderRowMenuIcon(AddCircleOutline),
    })
  }
  if (options.length > 0) {
    options.push({ type: 'divider', key: 'menu-divider' })
  }
  // 删除项着主题 error 色（公共件 errorOptionProps，注释见 row-menu-common.ts）。
  const errorProps = errorOptionProps(opts.errorColor)
  options.push({
    label: t('transactions.menu.delete'),
    key: 'delete',
    icon: renderRowMenuIcon(TrashOutline),
    ...errorProps,
  })
  return options
}
