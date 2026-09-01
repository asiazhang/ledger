// 交易列配置共享模块（Issue 39 Prefactor）：
// 交易列表与搜索视图复用同一列配置（日期/类型/分类/账户/备注/金额）。
// 渲染函数在运行时读取 store 的响应式数据，构建一次即可，无需 computed 包裹。

import { h, type VNode } from 'vue'
import { NTag, type DataTableColumn } from 'naive-ui'
import { formatAmount } from '@/types'
import type { Transaction, TransactionKind } from '@/types'
import type { useReferenceStore } from '@/stores/reference'
import { t } from '@/i18n'
import AccountLink from '@/components/AccountLink.vue'
import MerchantLink from '@/components/MerchantLink.vue'
import { lendingLabelKey, resolveLendingDirection } from '@/domain/lending'

export type ReferenceStore = ReturnType<typeof useReferenceStore>

const KIND_TAG_TYPE: Record<TransactionKind, 'success' | 'warning' | 'info' | 'default'> = {
  income: 'success',
  expense: 'warning',
  refund: 'info',
  transfer: 'default',
  buy: 'default',
  sell: 'default',
}

const AMOUNT_COLOR: Record<TransactionKind, string> = {
  income: '#18a058',
  expense: '#d03050',
  refund: '#2080f0',
  transfer: '',
  buy: '',
  sell: '',
}

/** 固定列宽总和（备注为弹性列不设 `width`，不计入）。
 * 作为 `scroll-x` 的窄窗口横向滚动下限。 */
export function sumFixedColumnWidths(columns: DataTableColumn<Transaction>[]): number {
  return columns.reduce(
    (sum, col) => sum + (typeof col.width === 'number' ? col.width : 0),
    0,
  )
}

/** 交易基础列：日期/类型/分类/账户/备注/金额（搜索结果与交易列表共用，只读）。
 * 列名经 t() 取当前语言：使用方以 computed 构造列数组（TransactionsView/SearchView），
 * 语言切换时重建列，表头即时更新。
 *
 * 列宽约定（Naive UI DataTable，headless Chrome 实测验证）：
 * - `ellipsis` 令 table-layout 强制为 fixed；fixed 布局下**未指定 `width` 的列均分剩余空间**，
 *   `minWidth`/`maxWidth` 均无效（maxWidth 仅 `resizable` 时生效）。
 * - 策略：除备注外所有列显式 `width`（贴合实际内容，不随窗口漂移）；**备注列不设 `width`，
 *   作为唯一弹性列吸收剩余空间**——窗口更宽则备注更宽、更窄则备注收缩，表格始终铺满容器，
 *   其余列不被挤压也不被拉伸。备注超长时由 `ellipsis` 省略 + 悬停全文。
 * - 不要覆盖 table 的 `width`（改 `auto` 会让带 `ellipsis` 的列被长文本撑宽，实测分类
 *   150→286px、备注 240→398px）。
 * - 使用方以「所有固定列（有 `width` 的列，含金额列；备注不计入）宽度总和」作为 `scroll-x`，
 *   作为窄窗口下的横向滚动下限。备注为弹性列，窗口变窄时先由备注收缩吸收，各固定列宽保持恒定——
 *   只有当内容区窄于固定列宽总和时才出现横向滚动（账户列 180、商户列 120 后固定列总和 825，窄窗口可能触发，
 *   由 scroll-x 提供横向滚动底线）。
 * - 宽度按实际内容估算：日期 105 / 类型 65 / 分类 150（最长路径 ≈149px）/ 商户 120 / 账户 180（转账行需容纳「转出 → 转入」两个账户名 + 箭头，长名由链接自身省略号兜底）/ 金额 125。 */
/** 类型标签（issue #374）：借贷是 transfer 的派生视角——两端账户类型构成借贷
 * （receivable/debt）的转账显示借出/收回/借入/还款专属文案，普通转账仍显示「转账」；
 * 非 transfer kind 不参与派生、按自身 kind 标签。历史数据实时派生、无数据迁移。
 * 方向识别收口 domain 层借贷模块（与表单分派/回填共用同一函数），
 * 标签随账户映射响应式更新（同 categoryPath 的响应式纪律）。 */
function kindLabel(reference: ReferenceStore, row: Transaction): string {
  if (row.kind !== 'transfer') return t(`transactions.kind.${row.kind}`)
  const direction = resolveLendingDirection(row, (id) => reference.accountMap.get(id)?.type)
  return t(lendingLabelKey(direction ?? 'none'))
}

export function buildTransactionColumns(reference: ReferenceStore): DataTableColumn<Transaction>[] {
  return [
    { title: t('transactions.columns.date'), key: 'date', width: 105 },
    {
      title: t('transactions.columns.kind'),
      key: 'kind',
      width: 65,
      render: (row) =>
        h(NTag, { type: KIND_TAG_TYPE[row.kind] }, () => kindLabel(reference, row)),
    },
    {
      title: t('transactions.columns.category'),
      key: 'category_id',
      width: 150,
      ellipsis: { tooltip: true },
      render: (row) => (row.category_id ? reference.categoryPath(row.category_id) || '-' : '-'),
    },
    {
      title: t('transactions.columns.merchant'),
      key: 'merchant_id',
      width: 120,
      ellipsis: { tooltip: true },
      // 商户名经 merchantMap（含软删）解析并可点击下钻（issue #191）；未知/无商户回退 '-'
      render: (row) =>
        row.merchant_id ? h(MerchantLink, { merchantId: row.merchant_id }) : '-',
    },
    {
      title: t('transactions.columns.account'),
      key: 'account_id',
      width: 180,
      render: (row) => renderAccountCell(row),
    },
    {
      title: t('transactions.columns.note'),
      key: 'note',
      // 弹性列：不设 width，由 fixed 布局均分剩余空间（超长时省略号 + 悬停显示全文）
      ellipsis: { tooltip: true },
      render: (row) => row.note ?? '-',
    },
    {
      title: t('transactions.columns.amount'),
      key: 'amount_native_cents',
      width: 125,
      render: (row) =>
        h(
          'span',
          { style: AMOUNT_COLOR[row.kind] ? `color: ${AMOUNT_COLOR[row.kind]}` : '' },
          formatAmount(row.amount_native_cents, reference.getCurrency(row.currency_code)),
        ),
    },
  ]
}

/** 转账单元格内账户链接的布局样式：flex 均分剩余宽度 + 允许收缩省略（长名各自省略号）。
 * 经 attrs 透传到 AccountLink 根按钮，与组件内部强调色样式合并。 */
const ACCOUNT_CELL_LINK_STYLE = 'flex: 1 1 0%; min-width: 0;'

/** 账户单元格渲染（issue #99）：
 * - 转账行显示「转出 → 转入」双向账户名（to_account_id 存在时），两个名字各自可点击、
 *   各自下钻到对应账户的过滤视图；
 * - 其余交易类型仍显示主账户名（可点击下钻，issue #97）。
 *
 * 布局：转账行用 inline-flex 容器，两个链接 flex 均分剩余宽度、箭头固定宽度；
 * 链接自身 ellipsis（见 AccountLink），长账户名省略号兜底。
 * 注意：该列不再设置列级 ellipsis（fixed 布局由备注列的 ellipsis 维持），
 * 否则 NEllipsis 会把两个按钮包装成整体省略，破坏各自可点击语义。 */
function renderAccountCell(row: Transaction): VNode {
  if (row.kind === 'transfer' && row.to_account_id) {
    return h(
      'div',
      {
        style: 'display: inline-flex; align-items: center; gap: 4px; width: 100%; max-width: 100%;',
      },
      [
        h(AccountLink, { accountId: row.account_id, style: ACCOUNT_CELL_LINK_STYLE }),
        h('span', { style: 'flex: none; opacity: 0.5;' }, '→'),
        h(AccountLink, { accountId: row.to_account_id, style: ACCOUNT_CELL_LINK_STYLE }),
      ],
    )
  }
  return h(AccountLink, { accountId: row.account_id })
}
