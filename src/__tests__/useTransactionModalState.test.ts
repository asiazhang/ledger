import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { invoke } from '@tauri-apps/api/core'
import { makeTransaction } from '@/__tests__/factories'
import { stubReferenceInvoke } from '@/__tests__/helpers/reference-stubs'
import { useTransactionModalState } from '@/composables/useTransactionModalState'
import type { TransactionTrade } from '@/types'

// 提示捕获（先例 useScheduledPlanList.test.ts）：setup.ts 的全局 useMessage mock
// 每次调用返回新对象，无法跨调用断言；此处以共享记录器覆盖（不渲染 naive-ui 组件，
// 其余导出保留原样）。useMessage 被 mock 为普通函数，工厂可在组件外直打。
const messageCalls: Array<{ method: string; text: string }> = []
vi.mock('naive-ui', async (importOriginal) => {
  const actual = await importOriginal<typeof import('naive-ui')>()
  const record =
    (method: string) =>
    (...args: unknown[]) =>
      messageCalls.push({ method, text: String(args[0]) })
  return {
    ...actual,
    useMessage: () => ({
      success: record('success'),
      warning: record('warning'),
      error: record('error'),
      info: record('info'),
      loading: record('loading'),
      destroyAll: () => {},
    }),
  }
})

const mockInvoke = vi.mocked(invoke)

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

beforeEach(() => {
  mockInvoke.mockReset()
  messageCalls.length = 0
  mockInvoke.mockImplementation((() =>
    Promise.reject(new Error('unexpected invoke'))) as typeof invoke)
})

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
    stubReferenceInvoke({
      get_transaction_trade: (args) =>
        args?.id === 'txn-b1'
          ? Promise.resolve(trade)
          : Promise.reject(new Error('unexpected invoke: get_transaction_trade')),
      list_insurers: [],
    })
    await modals.open({ type: 'edit', row })
    expect(mockInvoke).toHaveBeenCalledTimes(1)
    expect(mockInvoke.mock.calls[0]).toEqual(['get_transaction_trade', { id: 'txn-b1' }])
    expect(modals.intent.value).toEqual({ type: 'edit', row, trade })
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

  it('取明细失败：错误提示、不开窗（意图保持 null）、序号不递增', async () => {
    const modals = useTransactionModalState()
    const row = makeTransaction({ id: 'txn-bad', kind: 'buy' })
    mockInvoke.mockImplementation((() =>
      Promise.reject(new Error('数据库不可用'))) as typeof invoke)
    await modals.open({ type: 'edit', row })
    await flushPromises()
    expect(messageCalls).toEqual([{ method: 'error', text: '无法编辑: 数据库不可用' }])
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
    stubReferenceInvoke({
      get_transaction_trade: (args) => {
        if (args?.id === 'a1') return new Promise<TransactionTrade>((r) => (resolveA = r))
        if (args?.id === 'b1') return Promise.resolve(tradeB)
        return Promise.reject(new Error('unexpected invoke: get_transaction_trade'))
      },
      list_insurers: [],
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
    stubReferenceInvoke({
      get_transaction_trade: (args) => {
        if (args?.id === 'a1') return new Promise<TransactionTrade>((_, reject) => (rejectA = reject))
        if (args?.id === 'b1') return Promise.resolve(tradeB)
        return Promise.reject(new Error('unexpected invoke: get_transaction_trade'))
      },
      list_insurers: [],
    })

    const openA = modals.open({ type: 'edit', row: rowA })
    await modals.open({ type: 'edit', row: rowB })
    rejectA(new Error('数据库不可用'))
    await openA
    await flushPromises()
    expect(messageCalls).toEqual([])
    expect(modals.intent.value).toEqual({ type: 'edit', row: rowB, trade: tradeB })
    expect(modals.seq.value).toBe(1)
  })

  it('先快后慢（慢 B 后完成）：仍以最后 open 的 B 胜出，非先到先得', async () => {
    const modals = useTransactionModalState()
    const rowA = makeTransaction({ id: 'a1', kind: 'buy' })
    const rowB = makeTransaction({ id: 'b1', kind: 'buy' })
    const tradeB = makeTrade({ symbol: 'BBB' })
    let resolveB!: (trade: TransactionTrade) => void
    stubReferenceInvoke({
      get_transaction_trade: (args) => {
        if (args?.id === 'a1') return Promise.resolve(makeTrade({ symbol: 'AAA' }))
        if (args?.id === 'b1') return new Promise<TransactionTrade>((r) => (resolveB = r))
        return Promise.reject(new Error('unexpected invoke: get_transaction_trade'))
      },
      list_insurers: [],
    })

    await modals.open({ type: 'edit', row: rowA })
    expect(modals.intent.value!.row.id).toBe('a1')
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
    stubReferenceInvoke({
      get_transaction_trade: (args) =>
        args?.id === 'a1'
          ? new Promise<TransactionTrade>((r) => (resolveA = r))
          : Promise.reject(new Error('unexpected invoke: get_transaction_trade')),
      list_insurers: [],
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
    stubReferenceInvoke({
      get_transaction_trade: (args) =>
        args?.id === 'a1'
          ? new Promise<TransactionTrade>((r) => (resolveA = r))
          : Promise.reject(new Error('unexpected invoke: get_transaction_trade')),
      list_insurers: [],
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
