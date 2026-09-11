import { describe, expect, it } from 'vitest'
import { mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { flushPromises } from '@vue/test-utils'
import { makeTransaction } from '@/__tests__/factories'
import { messageCalls } from './helpers/message-mock'
import { useTransactionModalState } from '@/composables/useTransactionModalState'
import type { TransactionConvert, TransactionSplit, TransactionTrade } from '@/types'

// ---------------------------------------------------------------------------
// 数据工厂：买卖明细（交易行走共享 makeTransaction，factories.ts）
// ---------------------------------------------------------------------------

function makeTrade(overrides: Partial<TransactionTrade> = {}): TransactionTrade {
  return {
    instrument_id: 'inst-1',
    symbol: '600000',
    instrument_name: '浦发银行',
    instrument_type: 'stock',
    quantity: 100,
    price_cents: 1200,
    fee_cents: 500,
    ...overrides,
  }
}

describe('useTransactionModalState 初始状态', () => {
  it('意图为 null（= 关闭终态，显示开关由「意图非空」派生）、序号 0', () => {
    const modals = useTransactionModalState()
    expect(modals.intent.value).toBeNull()
    expect(modals.seq.value).toBe(0)
  })
})

describe('useTransactionModalState 同步意图（create / refund / add-item）', () => {
  it('open create：意图终态携子类型、序号递增；不经任何命令', async () => {
    const modals = useTransactionModalState()
    await modals.open({ type: 'create', kind: 'expense' })
    expect(modals.intent.value).toEqual({ type: 'create', kind: 'expense' })
    expect(modals.seq.value).toBe(1)
    expect(mockInvoke).not.toHaveBeenCalled()
  })

  it('换类型重开：kind 更新为最新意图、序号继续随 open 递增', async () => {
    const modals = useTransactionModalState()
    await modals.open({ type: 'create', kind: 'expense' })
    await modals.open({ type: 'create', kind: 'income' })
    expect(modals.intent.value).toEqual({ type: 'create', kind: 'income' })
    expect(modals.seq.value).toBe(2)
  })

  it('open refund：携目标行（开启时传入的引用），序号递增', async () => {
    const modals = useTransactionModalState()
    const row = makeTransaction({ id: 'txn-1' })
    await modals.open({ type: 'refund', row })
    expect(modals.intent.value).toEqual({ type: 'refund', row })
    expect(modals.intent.value!.type).toBe('refund')
    expect(modals.seq.value).toBe(1)
    expect(mockInvoke).not.toHaveBeenCalled()
  })

  it('open add-item：携目标行，序号递增', async () => {
    const modals = useTransactionModalState()
    const row = makeTransaction({ id: 'txn-2' })
    await modals.open({ type: 'add-item', row })
    expect(modals.intent.value).toEqual({ type: 'add-item', row })
    expect(modals.seq.value).toBe(1)
    expect(mockInvoke).not.toHaveBeenCalled()
  })
})

describe('useTransactionModalState edit 意图（先取明细再开窗）', () => {
  it.each(['buy', 'sell'] as const)('%s 行：先取买卖明细再开窗，明细随意图终态携带', async (kind) => {
    const modals = useTransactionModalState()
    const row = makeTransaction({ id: 'txn-b1', kind })
    const trade = makeTrade()
    wireInvokeSeam({
      overrides: {
        get_transaction_trade: (args) =>
        args?.id === 'txn-b1'
          ? Promise.resolve(trade)
          : Promise.reject(new Error('unexpected invoke: get_transaction_trade')),
      },
    })
    await modals.open({ type: 'edit', row })
    expect(mockInvoke).toHaveBeenCalledTimes(1)
    expect(mockInvoke.mock.calls[0]).toEqual(['get_transaction_trade', { id: 'txn-b1' }])
    expect(modals.intent.value).toEqual({ type: 'edit', row, trade })
    expect(modals.seq.value).toBe(1)
  })

  it('convert 行详情：先取转换两腿明细再开窗（get_transaction_convert），意图为 detail', async () => {
    const modals = useTransactionModalState()
    const row = makeTransaction({ id: 'txn-cv', kind: 'convert' })
    const convert: TransactionConvert = {
      out_instrument_id: 'inst-out',
      out_symbol: '006793',
      out_instrument_name: '转出基金',
      out_quantity: 10,
      out_amount_cents: 1_100,
      in_instrument_id: 'inst-in',
      in_symbol: '519700',
      in_instrument_name: '转入基金',
      in_quantity: 10,
      in_amount_cents: 900,
      fee_cents: 0,
      carried_cost_cents: 1_000,
      currency_code: 'CNY',
    }
    wireInvokeSeam({
      overrides: {
        get_transaction_convert: (args) =>
          args?.id === 'txn-cv'
            ? Promise.resolve(convert)
            : Promise.reject(new Error('unexpected invoke: get_transaction_convert')),
      },
    })
    await modals.open({ type: 'detail', row })
    expect(mockInvoke.mock.calls[0]).toEqual(['get_transaction_convert', { id: 'txn-cv' }])
    expect(modals.intent.value).toEqual({ type: 'detail', row, detail: { kind: 'convert', convert } })
    expect(modals.seq.value).toBe(1)
  })

  it('split 行详情：先取份额调整明细再开窗（get_transaction_split），意图为 detail', async () => {
    const modals = useTransactionModalState()
    const row = makeTransaction({ id: 'txn-sp', kind: 'split' })
    const split: TransactionSplit = {
      instrument_id: 'inst-sp',
      symbol: '502010',
      instrument_name: '证券基金',
      quantity: 339.76,
    }
    wireInvokeSeam({
      overrides: {
        get_transaction_split: (args) =>
          args?.id === 'txn-sp'
            ? Promise.resolve(split)
            : Promise.reject(new Error('unexpected invoke: get_transaction_split')),
      },
    })
    await modals.open({ type: 'detail', row })
    expect(mockInvoke.mock.calls[0]).toEqual(['get_transaction_split', { id: 'txn-sp' }])
    expect(modals.intent.value).toEqual({ type: 'detail', row, detail: { kind: 'split', split } })
    expect(modals.seq.value).toBe(1)
  })

  it('非买卖行：开窗即开（trade 为 null），不取明细', async () => {
    const modals = useTransactionModalState()
    const row = makeTransaction({ id: 'txn-1', kind: 'expense' })
    await modals.open({ type: 'edit', row })
    expect(mockInvoke).not.toHaveBeenCalled()
    expect(modals.intent.value).toEqual({ type: 'edit', row, trade: null })
    expect(modals.seq.value).toBe(1)
  })

  it('detail 非 convert / split 行：无详情面，不落意图（「意图非空即显示」不变式）', async () => {
    const modals = useTransactionModalState()
    const row = makeTransaction({ id: 'txn-1', kind: 'expense' })
    await modals.open({ type: 'detail', row })
    expect(mockInvoke).not.toHaveBeenCalled()
    expect(modals.intent.value).toBeNull()
    expect(modals.seq.value).toBe(0)
  })

  it('取明细失败：错误提示、不开窗（意图保持 null）、序号不递增', async () => {
    const modals = useTransactionModalState()
    const row = makeTransaction({ id: 'txn-bad', kind: 'buy' })
    wireInvokeSeam({
      overrides: {
        get_transaction_trade: () => Promise.reject(new Error('数据库不可用')),
      },
    })
    await modals.open({ type: 'edit', row })
    await flushPromises()
    expect(messageCalls()).toEqual([{ method: 'error', text: '无法编辑: 数据库不可用' }])
    expect(modals.intent.value).toBeNull()
    expect(modals.seq.value).toBe(0)
  })
})

describe('useTransactionModalState 竞态守卫（last-open-wins）', () => {
  it('慢 A + 快 B：终态停在 B，A 的迟到明细被丢弃（不覆盖意图、不再递增序号）', async () => {
    const modals = useTransactionModalState()
    const rowA = makeTransaction({ id: 'a1', kind: 'buy' })
    const rowB = makeTransaction({ id: 'b1', kind: 'buy' })
    const tradeA = makeTrade({ symbol: 'AAA' })
    const tradeB = makeTrade({ symbol: 'BBB' })
    let resolveA!: (trade: TransactionTrade) => void
    wireInvokeSeam({
      overrides: {
        get_transaction_trade: (args) => {
        if (args?.id === 'a1') return new Promise<TransactionTrade>((r) => (resolveA = r))
        if (args?.id === 'b1') return Promise.resolve(tradeB)
        return Promise.reject(new Error('unexpected invoke: get_transaction_trade'))
      },
      },
    })

    const openA = modals.open({ type: 'edit', row: rowA })
    await modals.open({ type: 'edit', row: rowB })
    expect(modals.intent.value).toEqual({ type: 'edit', row: rowB, trade: tradeB })
    expect(modals.seq.value).toBe(1)

    resolveA(tradeA)
    await openA
    await flushPromises()
    expect(modals.intent.value).toEqual({ type: 'edit', row: rowB, trade: tradeB })
    expect(modals.seq.value).toBe(1)
  })

  it('慢 A 失败 + 快 B 成功：A 迟到的失败整体丢弃（不提示错误），终态仍是 B', async () => {
    const modals = useTransactionModalState()
    const rowA = makeTransaction({ id: 'a1', kind: 'buy' })
    const rowB = makeTransaction({ id: 'b1', kind: 'buy' })
    const tradeB = makeTrade()
    let rejectA!: (e: Error) => void
    wireInvokeSeam({
      overrides: {
        get_transaction_trade: (args) => {
        if (args?.id === 'a1') return new Promise<TransactionTrade>((_, reject) => (rejectA = reject))
        if (args?.id === 'b1') return Promise.resolve(tradeB)
        return Promise.reject(new Error('unexpected invoke: get_transaction_trade'))
      },
      },
    })

    const openA = modals.open({ type: 'edit', row: rowA })
    await modals.open({ type: 'edit', row: rowB })
    rejectA(new Error('数据库不可用'))
    await openA
    await flushPromises()
    expect(messageCalls()).toEqual([])
    expect(modals.intent.value).toEqual({ type: 'edit', row: rowB, trade: tradeB })
    expect(modals.seq.value).toBe(1)
  })

  it('先快后慢（慢 B 后完成）：仍以最后 open 的 B 胜出，非先到先得', async () => {
    const modals = useTransactionModalState()
    const rowA = makeTransaction({ id: 'a1', kind: 'buy' })
    const rowB = makeTransaction({ id: 'b1', kind: 'buy' })
    const tradeB = makeTrade({ symbol: 'BBB' })
    let resolveB!: (trade: TransactionTrade) => void
    wireInvokeSeam({
      overrides: {
        get_transaction_trade: (args) => {
        if (args?.id === 'a1') return Promise.resolve(makeTrade({ symbol: 'AAA' }))
        if (args?.id === 'b1') return new Promise<TransactionTrade>((r) => (resolveB = r))
        return Promise.reject(new Error('unexpected invoke: get_transaction_trade'))
      },
      },
    })

    await modals.open({ type: 'edit', row: rowA })
    // 编辑意图已在场（trade 仍在途，下一断言补全）；matchObject 兼容意图联合（create 支无 row）
    expect(modals.intent.value).toMatchObject({ type: 'edit', row: { id: 'a1' } })
    const openB = modals.open({ type: 'edit', row: rowB })
    expect(modals.intent.value).toEqual({ type: 'edit', row: rowA, trade: makeTrade({ symbol: 'AAA' }) })

    resolveB(tradeB)
    await openB
    await flushPromises()
    expect(modals.intent.value).toEqual({ type: 'edit', row: rowB, trade: tradeB })
    expect(modals.seq.value).toBe(2)
  })

  it('取数在途时 close：迟到的成功不再重开弹窗（关闭是终态，清空后不被复活）', async () => {
    const modals = useTransactionModalState()
    const rowA = makeTransaction({ id: 'a1', kind: 'buy' })
    let resolveA!: (trade: TransactionTrade) => void
    wireInvokeSeam({
      overrides: {
        get_transaction_trade: (args) =>
        args?.id === 'a1'
          ? new Promise<TransactionTrade>((r) => (resolveA = r))
          : Promise.reject(new Error('unexpected invoke: get_transaction_trade')),
      },
    })

    const openA = modals.open({ type: 'edit', row: rowA })
    modals.close()
    resolveA(makeTrade())
    await openA
    await flushPromises()
    expect(modals.intent.value).toBeNull()
    expect(modals.seq.value).toBe(0)
  })

  it('同步意图不参与竞态：编辑取数在途时开同步意图即时生效，编辑迟到结果仍被丢弃', async () => {
    const modals = useTransactionModalState()
    const rowA = makeTransaction({ id: 'a1', kind: 'buy' })
    const rowR = makeTransaction({ id: 'r1', kind: 'expense' })
    let resolveA!: (trade: TransactionTrade) => void
    wireInvokeSeam({
      overrides: {
        get_transaction_trade: (args) =>
        args?.id === 'a1'
          ? new Promise<TransactionTrade>((r) => (resolveA = r))
          : Promise.reject(new Error('unexpected invoke: get_transaction_trade')),
      },
    })

    const openA = modals.open({ type: 'edit', row: rowA })
    await modals.open({ type: 'refund', row: rowR })
    expect(modals.intent.value!.type).toBe('refund')
    resolveA(makeTrade())
    await openA
    await flushPromises()
    expect(modals.intent.value!.type).toBe('refund')
    expect(modals.seq.value).toBe(1)
  })
})

describe('useTransactionModalState 关闭', () => {
  it('close：意图清回 null 终态；序号保持（关闭不递增）', async () => {
    const modals = useTransactionModalState()
    await modals.open({ type: 'create', kind: 'expense' })
    modals.close()
    expect(modals.intent.value).toBeNull()
    expect(modals.seq.value).toBe(1)
  })

  it('关闭后可重开：意图流转正常、序号继续递增', async () => {
    const modals = useTransactionModalState()
    await modals.open({ type: 'create', kind: 'expense' })
    modals.close()
    const row = makeTransaction({ id: 'txn-1' })
    await modals.open({ type: 'refund', row })
    expect(modals.intent.value).toEqual({ type: 'refund', row })
    expect(modals.seq.value).toBe(2)
  })

  it('未开启时 close 幂等：意图保持 null', () => {
    const modals = useTransactionModalState()
    modals.close()
    expect(modals.intent.value).toBeNull()
  })
})

describe('useTransactionModalState 工厂形态', () => {
  it('每次调用返回独立实例：意图与序号互不串扰', async () => {
    const first = useTransactionModalState()
    const second = useTransactionModalState()
    await first.open({ type: 'create', kind: 'expense' })
    expect(first.intent.value).toEqual({ type: 'create', kind: 'expense' })
    expect(first.seq.value).toBe(1)
    expect(second.intent.value).toBeNull()
    expect(second.seq.value).toBe(0)
    second.close()
    expect(first.intent.value).not.toBeNull()
  })
})
