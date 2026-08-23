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

/** 交易基础列：日期/类型/分类/账户/备注/金额（搜索结果与交易列表共用，只读）。 */
export function buildTransactionColumns(store: AppStore): DataTableColumn<Transaction>[] {
  return [
    { title: '日期', key: 'date', width: 120 },
    {
      title: '类型',
      key: 'kind',
      width: 80,
      render: (row) =>
        h(NTag, { type: KIND_TAG_TYPE[row.kind] }, () => TRANSACTION_KIND_LABELS[row.kind]),
    },
    {
      title: '分类',
      key: 'category_id',
      render: (row) => (row.category_id ? store.categoryPath(row.category_id) || '-' : '-'),
    },
    {
      title: '账户',
      key: 'account_id',
      render: (row) => store.accountMap.get(row.account_id)?.name ?? '无',
    },
    { title: '备注', key: 'note', render: (row) => row.note ?? '-' },
    {
      title: '金额',
      key: 'amount_native_cents',
      width: 140,
      render: (row) =>
        h(
          'span',
          { style: AMOUNT_COLOR[row.kind] ? `color: ${AMOUNT_COLOR[row.kind]}` : '' },
          formatAmount(row.amount_native_cents, store.getCurrency(row.currency_code)),
        ),
    },
  ]
}
