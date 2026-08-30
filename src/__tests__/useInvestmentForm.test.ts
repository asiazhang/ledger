import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import { useInvestmentForm } from '@/composables/useInvestmentForm'
import type { Account, Currency, Instrument, Transaction, TransactionTrade } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-inv', name: '证券户', type: 'investment', currency_code: 'CNY',
    initial_balance_cents: 0, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false, is_hidden: false,
  },
  {
    id: 'acc-cash', name: '现金', type: 'cash', currency_code: 'CNY',
    initial_balance_cents: 0, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false, is_hidden: false,
  },
]

const mockInstruments: Instrument[] = [
  {
    id: 'ins-1', symbol: 'NVDA', name: '英伟达', type: 'stock', currency_code: 'CNY',
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false, market: 'unknown', invested: false,
    source: 'eastmoney', price_cents: null,
  },
]

const editingTx: Transaction = {
  id: 'txn-buy-1',
  kind: 'buy',
  amount_cents: 15500,
  currency_code: 'CNY',
  amount_native_cents: 15500,
  account_id: 'acc-inv',
  to_account_id: null,
  category_id: null,
  refund_of_transaction_id: null,
  note: '建仓买入',
  date: '2026-01-10',
  created_at: '2026-01-10T01:00:00Z',
  updated_at: '2026-01-10T01:00:00Z',
  version: 1,
  device_id: 'test',
  is_deleted: false,
}

const editingTrade: TransactionTrade = {
  instrument_id: 'ins-1',
  symbol: 'NVDA',
  instrument_name: '英伟达',
  quantity: 100,
  price_cents: 1500000, // 150 元（万分之一元刻度）
  fee_cents: 500,
}

describe('useInvestmentForm', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_merchants') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
  })

  it('初始化状态：账户/标的/数量/价格为空', () => {
    const form = useInvestmentForm('buy')
    expect(form.accountId.value).toBeNull()
    expect(form.instrumentId.value).toBeNull()
    expect(form.quantity.value).toBeNull()
    expect(form.price.value).toBeNull()
  })

  it('submit 校验：无账户/标的/数量/单价时警告且不写入', async () => {
    const form = useInvestmentForm('buy')
    await form.submit()
    form.accountId.value = 'acc-inv'
    await form.submit()
    form.instrumentId.value = 'ins-1'
    await form.submit()
    form.quantity.value = 10
    await form.submit()
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction'),
    ).toHaveLength(0)
  })

  it('submit 创建：调用 create_transaction，成功后重置表单', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_merchants') return Promise.resolve([])
      if (cmd === 'create_transaction') return Promise.resolve('new-txn')
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const onCreated = vi.fn()
    const form = useInvestmentForm('buy', { onCreated })
    form.accountId.value = 'acc-inv'
    form.instrumentId.value = 'ins-1'
    form.quantity.value = 10
    form.price.value = 15
    form.fee.value = 5
    form.note.value = '测试'
    form.date.value = new Date('2026-07-11').getTime()

    await form.submit()

    // 提交路由：创建命令 + 正确 kind；wire 字段形状（含 buy 占位语义）由装配器
    // 测试承担（issue #216）
    expect(mockInvoke).toHaveBeenCalledWith('create_transaction', {
      input: expect.objectContaining({ kind: 'buy' }),
    })
    // 创建成功后重置业务字段
    expect(form.instrumentId.value).toBeNull()
    expect(form.quantity.value).toBeNull()
    expect(onCreated).toHaveBeenCalledTimes(1)
  })

  describe('编辑模式（issue #180）', () => {
    it('创建即回填：账户/标的/数量/价格/费用/备注/日期/币种，标的候选项含回填标的（显示 symbol · name）', async () => {
      const store = useReferenceStore()
      await store.refresh()
      const form = useInvestmentForm('buy', {
        editing: () => editingTx,
        trade: () => editingTrade,
      })
      expect(form.accountId.value).toBe('acc-inv')
      expect(form.instrumentId.value).toBe('ins-1')
      expect(form.quantity.value).toBe(100)
      expect(form.price.value).toBe(150)
      expect(form.fee.value).toBe(5)
      expect(form.note.value).toBe('建仓买入')
      expect(form.date.value).toBe(new Date('2026-01-10T00:00:00Z').getTime())
      expect(form.currencyCode.value).toBe('CNY')
      // 远程搜索未执行（无候选）时，回填标的仍可显示
      expect(form.instrumentOptions.value).toEqual([
        { label: 'NVDA · 英伟达', value: 'ins-1' },
      ])
    })

    it('回填标的名称为空时候选 label 仅显示 symbol；用户搜索不冲掉回填标的选项', async () => {
      const store = useReferenceStore()
      await store.refresh()
      const form = useInvestmentForm('buy', {
        editing: () => editingTx,
        trade: () => ({ ...editingTrade, instrument_name: null }),
      })
      expect(form.instrumentOptions.value).toEqual([{ label: 'NVDA', value: 'ins-1' }])
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
        if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
        if (cmd === 'list_categories') return Promise.resolve([])
        if (cmd === 'list_merchants') return Promise.resolve([])
        if (cmd === 'list_instruments') return Promise.resolve({ items: mockInstruments, total: 1 })
        return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
      })
      form.searchInstruments('NVDA')
      await vi.waitFor(() => {
        expect(form.searchingInstruments.value).toBe(false)
      })
      // 搜索结果在前、回填标的（已含于结果则不重复）合并展示
      expect(form.instrumentOptions.value.map((o) => o.value)).toEqual(['ins-1'])
    })

    it('submit 编辑：分派 update_transaction（同一入参形状），onUpdated 触发、onCreated 不触发、不重置表单', async () => {
      const store = useReferenceStore()
      await store.refresh()
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
        if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
        if (cmd === 'list_categories') return Promise.resolve([])
        if (cmd === 'list_merchants') return Promise.resolve([])
        if (cmd === 'update_transaction') return Promise.resolve(null)
        return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
      })
      const onCreated = vi.fn()
      const onUpdated = vi.fn()
      const form = useInvestmentForm('buy', {
        onCreated,
        onUpdated,
        editing: () => editingTx,
        trade: () => editingTrade,
      })

      await form.submit()

      // 提交路由：更新命令携带交易 id + 回填业务字段交接装配结果；
      // 金额/日期转换与占位字段由装配器测试承担（issue #216）
      expect(mockInvoke).toHaveBeenCalledWith('update_transaction', {
        id: 'txn-buy-1',
        input: expect.objectContaining({
          kind: 'buy',
          instrument_id: 'ins-1',
          quantity: 100,
          note: '建仓买入',
        }),
      })
      expect(onUpdated).toHaveBeenCalledTimes(1)
      expect(onCreated).not.toHaveBeenCalled()
      // 编辑路径不重置表单：成功即关窗（onUpdated），实例整体销毁
      expect(form.instrumentId.value).toBe('ins-1')
      expect(form.quantity.value).toBe(100)
    })

    it('submit 编辑失败：错误不抛出、onUpdated 不触发、已填内容不丢', async () => {
      const store = useReferenceStore()
      await store.refresh()
      const onUpdated = vi.fn()
      const form = useInvestmentForm('buy', {
        onUpdated,
        editing: () => editingTx,
        trade: () => editingTrade,
      })
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
        if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
        if (cmd === 'list_categories') return Promise.resolve([])
        if (cmd === 'list_merchants') return Promise.resolve([])
        if (cmd === 'update_transaction') return Promise.reject(new Error('该买入交易已有部分卖出，无法修改'))
        return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
      })

      await expect(form.submit()).resolves.toBeUndefined()
      expect(onUpdated).not.toHaveBeenCalled()
      expect(form.instrumentId.value).toBe('ins-1')
      expect(form.quantity.value).toBe(100)
      expect(form.price.value).toBe(150)
    })
  })
})
