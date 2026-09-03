import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { DataTableColumn } from 'naive-ui'
import type { VNode } from 'vue'
import {
  buildTransactionColumns,
  type ReferenceStore,
} from '@/components/transaction-columns'
import { useAppStore } from '@/stores/app'
import { kindSemanticColor } from '@/theme/semantic-colors'
import { TRANSACTION_KINDS, type Transaction } from '@/types'
import { makeTransaction } from './factories'

/** 金额列按交易类型语义着色（issue #435）：只测外部行为——
 * 给定交易类型与主题，金额单元格最终呈现语义色模块给出的颜色；
 * 模块自身的色值定案见 semantic-colors.test.ts。 */

const reference = {
  categoryPath: () => null,
  accountMap: new Map(),
  getCurrency: () => undefined,
} as unknown as ReferenceStore

function amountStyleOf(row: Transaction): string {
  const columns = buildTransactionColumns(reference)
  const amountColumn = columns.find(
    (c): c is DataTableColumn<Transaction> & { render: NonNullable<DataTableColumn<Transaction>['render']> } =>
      c.key === 'amount_native_cents',
  )!
  const vnode = amountColumn.render(row, 0) as VNode
  return (vnode.props as { style?: string }).style ?? ''
}

describe('buildTransactionColumns 金额单元格语义着色', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('暗色主题（默认）：逐类型呈现语义色暗色变体', () => {
    const app = useAppStore()
    app.setTheme('dark')
    for (const kind of TRANSACTION_KINDS) {
      expect(amountStyleOf(makeTransaction({ id: `tx-${kind}`, kind })), kind).toBe(
        `color: ${kindSemanticColor(kind, 'dark')}`,
      )
    }
  })

  it('亮色主题：逐类型呈现语义色亮色变体（收入绿/支出红/退款蓝维持既有亮色值）', () => {
    const app = useAppStore()
    app.setTheme('light')
    for (const kind of TRANSACTION_KINDS) {
      expect(amountStyleOf(makeTransaction({ id: `tx-${kind}`, kind })), kind).toBe(
        `color: ${kindSemanticColor(kind, 'light')}`,
      )
    }
  })

  it('切换主题即时换色：同一列配置下重渲染即取新主题色（无需重建列）', () => {
    const app = useAppStore()
    app.setTheme('dark')
    const columns = buildTransactionColumns(reference)
    const row = makeTransaction({ id: 'tx-expense', kind: 'expense' })
    const render = columns.find((c) => c.key === 'amount_native_cents')!.render!
    const colorOf = () => ((render(row, 0) as VNode).props as { style: string }).style
    const darkStyle = colorOf()
    app.setTheme('light')
    expect(colorOf()).not.toBe(darkStyle)
    expect(colorOf()).toBe(`color: ${kindSemanticColor('expense', 'light')}`)
  })
})
