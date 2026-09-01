import { CreateOutline, SwapHorizontalOutline, TrashOutline } from '@vicons/ionicons5'
import type { DropdownOption } from 'naive-ui'
import { t } from '@/i18n'
import { errorOptionProps, renderRowMenuIcon } from './row-menu-common'

/**
 * 账户行菜单选项组装（编辑 / 调整余额 / 删除）：纯函数收口，菜单形状可独立测试
 * （与交易行菜单 buildRowMenuOptions 同一模式）。
 *
 * 菜单形状：编辑（CreateOutline）/ 调整余额（SwapHorizontalOutline，调整即与黑洞
 * 账户的转账，ADR-0026）/ 分隔线 / 删除（TrashOutline，着主题 error 色）。
 *
 * `errorColor`：当前主题的 error 色（组件经 useThemeVars 取值传入）。
 */
export function buildAccountRowMenuOptions(
  opts: { errorColor?: string } = {},
): DropdownOption[] {
  return [
    { label: t('accounts.menu.edit'), key: 'edit', icon: renderRowMenuIcon(CreateOutline) },
    {
      label: t('accounts.menu.adjustBalance'),
      key: 'adjust-balance',
      icon: renderRowMenuIcon(SwapHorizontalOutline),
    },
    { type: 'divider', key: 'menu-divider' },
    {
      label: t('accounts.menu.delete'),
      key: 'delete',
      icon: renderRowMenuIcon(TrashOutline),
      ...errorOptionProps(opts.errorColor),
    },
  ]
}
