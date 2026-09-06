import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { DataTableColumn } from 'naive-ui'
import type { VNode } from 'vue'
import {
  buildTransactionColumns,
  type ReferenceStore,
} from '@/components/transaction-columns'
import SourceLink from '@/components/SourceLink.vue'
import { useAppStore } from '@/stores/app'
import { kindSemanticColor } from '@/theme/semantic-colors'
import { TRANSACTION_KINDS, type Transaction, type TransactionSource } from '@/types'
import { makeTransaction } from './factories'

/** 金额列按交易类型语义着色（issue #435）：只测外部行为——
 * 给定交易类型与主题，金额单元格最终呈现语义色模块给出的颜色；
 * 模块自身的色值定案见 semantic-colors.test.ts。 */

const reference = {
  categoryPath: () => null,
  accountMap: new Map(),
  getCurrency: () => undefined,
} as unknown as ReferenceStore

/**
 * 按键取渲染列：DataTableColumn 是含分组列的联合（key/render 并非每支都有），
 * 本文件只消费「带键、带 render 的普通列」——经断言守卫单点窄化，
 * 不在用例内散布 as/非空断言。
 */
function renderColumnOf(columns: DataTableColumn<Transaction>[], key: string) {
  const hit = columns.find((c) => (c as { key?: unknown }).key === key)
  expect(hit, `列 ${key} 应存在`).toBeTruthy()
  const render = (hit as { render?: unknown }).render
  expect(typeof render).toBe('function')
  return render as (row: Transaction, index: number) => unknown
}

function amountStyleOf(row: Transaction): string {
  const render = renderColumnOf(buildTransactionColumns(reference), 'amount_native_cents')
  const vnode = render(row, 0) as VNode
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
    const row = makeTransaction({ id: 'tx-expense', kind: 'expense' })
    const render = renderColumnOf(buildTransactionColumns(reference), 'amount_native_cents')
    const colorOf = () => ((render(row, 0) as VNode).props as { style: string }).style
    const darkStyle = colorOf()
    app.setTheme('light')
    expect(colorOf()).not.toBe(darkStyle)
    expect(colorOf()).toBe(`color: ${kindSemanticColor('expense', 'light')}`)
  })
})

/** 来源列（spec #704 / issue #706）：列序与渲染产物——
 * 只测外部行为：列位置、单元格产物（SourceLink 组件/占位符），
 * 链接交互与状态标注的渲染矩阵归 SourceLink 组件测试（一缝一测）。 */
describe('buildTransactionColumns 来源列', () => {
  function columnByKey(key: string) {
    return renderColumnOf(buildTransactionColumns(reference), key)
  }

  function sourceCellOf(row: Transaction) {
    return columnByKey('source')(row, 0)
  }

  it('列序：来源列位于账户之后、备注之前', () => {
    const keys = buildTransactionColumns(reference).map((c) => (c as { key?: string }).key)
    expect(keys.indexOf('account_id')).toBeLessThan(keys.indexOf('source'))
    expect(keys.indexOf('source')).toBeLessThan(keys.indexOf('note'))
  })

  it('保单来源渲染 SourceLink，携带行来源对象', () => {
    const source: TransactionSource = {
      kind: 'policy',
      entity_id: 'pol-1',
      display_name: '重疾险',
      status: null,
    }
    const vnode = sourceCellOf(makeTransaction({ id: 't1', source })) as VNode
    expect(vnode.type).toBe(SourceLink)
    expect((vnode.props as { source: TransactionSource }).source).toEqual(source)
  })

  it('软删保单来源同样走 SourceLink（禁用点击/标注归组件渲染矩阵）', () => {
    const source: TransactionSource = {
      kind: 'policy',
      entity_id: 'pol-2',
      display_name: '医疗险',
      status: 'deleted',
    }
    const vnode = sourceCellOf(makeTransaction({ id: 't2', source })) as VNode
    expect(vnode.type).toBe(SourceLink)
  })

  it('无来源留空（占位符，手动/AI 导入口径）', () => {
    expect(sourceCellOf(makeTransaction({ id: 't3' }))).toBe('-')
  })
})
