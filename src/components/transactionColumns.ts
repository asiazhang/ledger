// 交易列配置共享模块（Issue 39 Prefactor）：
// 交易列表与搜索视图复用同一列配置（日期/类型/分类/账户/备注/金额）。
// 渲染函数在运行时读取 store 的响应式数据，构建一次即可，无需 computed 包裹。

import { h } from 'vue'
import { NTag, type DataTableColumn } from 'naive-ui'
import { formatAmount, TRANSACTION_KIND_LABELS } from '@/types'
import type { Transaction, TransactionKind } from '@/types'
import type { useAppStore } from '@/stores/app'

export type AppStore = ReturnType<typeof useAppStore>

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
 *
 * 列宽约定（Naive UI DataTable，headless Chrome 实测验证）：
 * - `ellipsis` 令 table-layout 强制为 fixed；fixed 布局下**未指定 `width` 的列均分剩余空间**，
 *   `minWidth`/`maxWidth` 均无效（maxWidth 仅 `resizable` 时生效）。
 * - 策略：除备注外所有列显式 `width`（贴合实际内容，不随窗口漂移）；**备注列不设 `width`，
 *   作为唯一弹性列吸收剩余空间**——窗口更宽则备注更宽、更窄则备注收缩，表格始终铺满容器，
 *   其余列不被挤压也不被拉伸。备注超长时由 `ellipsis` 省略 + 悬停全文。
 * - 不要覆盖 table 的 `width`（改 `auto` 会让带 `ellipsis` 的列被长文本撑宽，实测分类
 *   150→286px、备注 240→398px）。
 * - 使用方以「所有固定列（有 `width` 的列，含金额/操作列；备注不计入）宽度总和」作为 `scroll-x`，
 *   作为窄窗口下的横向滚动下限。备注为弹性列，窗口变窄时先由备注收缩吸收，各固定列宽保持恒定——
 *   只有当内容区窄于固定列宽总和时才出现横向滚动（最小窗口内容区 660 > 固定列总和 645，故通常不触发）。
 * - 宽度按实际内容估算：日期 105 / 类型 65 / 分类 150（最长路径 ≈149px）/ 账户 120（最长 ≈86px）/ 金额 125。 */
export function buildTransactionColumns(store: AppStore): DataTableColumn<Transaction>[] {
  return [
    { title: '日期', key: 'date', width: 105 },
    {
      title: '类型',
      key: 'kind',
      width: 65,
      render: (row) =>
        h(NTag, { type: KIND_TAG_TYPE[row.kind] }, () => TRANSACTION_KIND_LABELS[row.kind]),
    },
    {
      title: '分类',
      key: 'category_id',
      width: 150,
      ellipsis: { tooltip: true },
      render: (row) => (row.category_id ? store.categoryPath(row.category_id) || '-' : '-'),
    },
    {
      title: '账户',
      key: 'account_id',
      width: 120,
      ellipsis: { tooltip: true },
      render: (row) => store.accountMap.get(row.account_id)?.name ?? '无',
    },
    {
      title: '备注',
      key: 'note',
      // 弹性列：不设 width，由 fixed 布局均分剩余空间（超长时省略号 + 悬停显示全文）
      ellipsis: { tooltip: true },
      render: (row) => row.note ?? '-',
    },
    {
      title: '金额',
      key: 'amount_native_cents',
      width: 125,
      render: (row) =>
        h(
          'span',
          { style: AMOUNT_COLOR[row.kind] ? `color: ${AMOUNT_COLOR[row.kind]}` : '' },
          formatAmount(row.amount_native_cents, store.getCurrency(row.currency_code)),
        ),
    },
  ]
}
