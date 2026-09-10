import { describe, expect, it, vi } from 'vitest'
import { NEllipsis, type DataTableColumn } from 'naive-ui'
import type { VNode } from 'vue'
import {
  buildTransactionColumns,
  type ReferenceStore,
} from '@/components/transaction-columns'
import SourceLink from '@/components/SourceLink.vue'
import NoteCopyButton from '@/components/NoteCopyButton.vue'
import AmountCell from '@/components/AmountCell.vue'
import { useAppStore } from '@/stores/app'
import { kindSemanticColor } from '@/theme/semantic-colors'
import { TRANSACTION_KINDS, type Transaction, type TransactionSource } from '@/types'
import { formatAmount } from '@/utils/money'
import { makeTransaction } from './factories'

/** 金额列按交易类型语义着色（issue #435）：只测外部行为——
 * 给定交易类型与主题，金额单元格最终呈现语义色模块给出的颜色；
 * 模块自身的色值定案见 semantic-colors.test.ts。
 * 单元格交互归 AmountCell 组件（issue #843），此处只断言列配置的载荷单点。 */

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

function amountCellOf(row: Transaction): VNode {
  const render = renderColumnOf(buildTransactionColumns(reference), 'amount_native_cents')
  return render(row, 0) as VNode
}

describe('buildTransactionColumns 金额单元格语义着色', () => {
  it('暗色主题（默认）：逐类型呈现语义色暗色变体', () => {
    const app = useAppStore()
    app.setTheme('dark')
    for (const kind of TRANSACTION_KINDS) {
      const vnode = amountCellOf(makeTransaction({ id: `tx-${kind}`, kind }))
      expect(vnode.type).toBe(AmountCell)
      expect((vnode.props as { color: string }).color, kind).toBe(
        kindSemanticColor(kind, 'dark'),
      )
    }
  })

  it('亮色主题：逐类型呈现语义色亮色变体（收入绿/支出红/退款蓝维持既有亮色值）', () => {
    const app = useAppStore()
    app.setTheme('light')
    for (const kind of TRANSACTION_KINDS) {
      const vnode = amountCellOf(makeTransaction({ id: `tx-${kind}`, kind }))
      expect((vnode.props as { color: string }).color, kind).toBe(
        kindSemanticColor(kind, 'light'),
      )
    }
  })

  it('切换主题即时换色：同一列配置下重渲染即取新主题色（无需重建列）', () => {
    const app = useAppStore()
    app.setTheme('dark')
    const row = makeTransaction({ id: 'tx-expense', kind: 'expense' })
    const colorOf = () => (amountCellOf(row).props as { color: string }).color
    const darkStyle = colorOf()
    app.setTheme('light')
    expect(colorOf()).not.toBe(darkStyle)
    expect(colorOf()).toBe(kindSemanticColor('expense', 'light'))
  })

  it('金额文案单点：单元格载荷携带 formatAmount 产物（含币种形态，issue #843 AmountCell 载荷）', () => {
    const row = makeTransaction({ id: 'tx-cny', amount_native_cents: 123456 })
    const vnode = amountCellOf(row)
    expect((vnode.props as { text: string }).text).toBe(
      formatAmount(123456, reference.getCurrency(row.currency_code)),
    )
  })

  it('转换行金额显示转出金额（确认单口径），不读行金额锚点（结转成本，ADR-0099）', () => {
    const app = useAppStore()
    app.setTheme('dark')
    const row = makeTransaction({
      id: 'tx-convert',
      kind: 'convert',
      // 行金额锚点 = 结转成本（3590.62），展示口径 = 转出金额（3615.61）。
      amount_native_cents: 359062,
      convert: {
        to_instrument_id: 'inst-in',
        to_quantity: 10,
        out_amount_cents: 361561,
        in_amount_cents: 361561,
      },
    })
    const vnode = amountCellOf(row)
    expect((vnode.props as { text: string }).text).toBe(
      formatAmount(361561, reference.getCurrency(row.currency_code)),
    )
    expect((vnode.props as { color: string }).color).toBe(kindSemanticColor('convert', 'dark'))
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

/** 备注列（显式复制通道，见「界面文本不可选」词条）：只测单元格产物——
 * 占位符/容器结构/按钮载荷；复制动作与 toast 归 NoteCopyButton 组件测试（一缝一测）。 */
describe('buildTransactionColumns 备注列', () => {
  function noteCellOf(row: Transaction) {
    const render = renderColumnOf(buildTransactionColumns(reference), 'note')
    return render(row, 0)
  }

  it('无备注渲染占位符，不渲染复制按钮（空备注无可复制）', () => {
    expect(noteCellOf(makeTransaction({ id: 't1', note: null }))).toBe('-')
  })

  it('有备注渲染单元格容器：文本 NEllipsis（自省略+悬停全文）与复制按钮并排，按钮携带完整备注', () => {
    const vnode = noteCellOf(makeTransaction({ id: 't2', note: '视频会员月费' })) as VNode
    expect((vnode.props as { style: string }).style).toContain('display: flex')
    const children = vnode.children as VNode[]
    expect(children).toHaveLength(2)
    expect(children[0].type).toBe(NEllipsis)
    expect(children[1].type).toBe(NoteCopyButton)
    expect((children[1].props as { note: string }).note).toBe('视频会员月费')
  })
})

/** 交易行「⋯」常显操作列（ADR-0088 决策 6 / issue #843）：只测装配面——
 * 回调在场才追加列、菜单打开回调随按钮携行；菜单项集合与动作分派归
 * TransactionsView 组件测试（行菜单编排接缝，一缝一测）。 */
describe('buildTransactionColumns 操作列（「⋯」常显第二入口）', () => {
  const row = makeTransaction({ id: 't-actions', kind: 'expense' })

  it('未声明 onRowMenuOpen 不追加操作列（搜索结果保持只读）', () => {
    const keys = buildTransactionColumns(reference).map((c) => (c as { key?: string }).key)
    expect(keys).not.toContain('actions')
  })

  it('声明 onRowMenuOpen 追加末位操作列：tiny「⋯」按钮，点击以事件与目标行回调', () => {
    const onRowMenuOpen = vi.fn()
    const columns = buildTransactionColumns(reference, { onRowMenuOpen })
    const actions = columns.find((c) => (c as { key?: string }).key === 'actions')
    expect(actions).toBeTruthy()
    const keys = columns.map((c) => (c as { key?: string }).key)
    expect(keys[keys.length - 1]).toBe('actions')
    const vnode = (actions as unknown as { render: (row: Transaction) => VNode }).render(row)
    const children = vnode.children as { default: () => string }
    expect(children.default()).toBe('⋯')
    const props = vnode.props as {
      class: string
      'aria-label': string
      onClick: (e: MouseEvent) => void
    }
    expect(props.class).toBe('row-actions-btn touch-hit-area')
    expect(props['aria-label']).toBe('更多操作')
    const event = new MouseEvent('click', { clientX: 10, clientY: 20 })
    props.onClick(event)
    expect(onRowMenuOpen).toHaveBeenCalledWith(event, row)
  })
})
