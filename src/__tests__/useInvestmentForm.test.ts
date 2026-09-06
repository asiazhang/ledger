import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { setActivePinia, createPinia } from 'pinia'
import { useReferenceStore } from '@/stores/reference'
import { useInvestmentForm } from '@/composables/useInvestmentForm'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import type { Account, Instrument, Transaction, TransactionTrade } from '@/types'


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

const mockFundInstruments: Instrument[] = [
  {
    id: 'ins-fund', symbol: '000123', name: '某混合基金', type: 'fund', currency_code: 'CNY',
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
  merchant_id: null,
  policy_id: null,
  source: null,
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
  instrument_type: 'stock',
  quantity: 100,
  price_cents: 1500000, // 150 元（万分之一元刻度）
  fee_cents: 500,
}

const editingFundTx: Transaction = {
  ...editingTx,
  id: 'txn-fund-1',
  amount_cents: 100000, // 确认单整分金额 1000 元（权威）
}

const editingFundTrade: TransactionTrade = {
  instrument_id: 'ins-fund',
  symbol: '000123',
  instrument_name: '某混合基金',
  instrument_type: 'fund',
  quantity: 987.6543,
  price_cents: 10110, // 反算净值 1.0110 元（万分之一元刻度）
  fee_cents: 150,
}

/** beforeEach 主链派发函数：中途重桩处理完自己的领域命令后委托回它 */
let base: ReturnType<typeof stubReferenceInvoke>

describe('useInvestmentForm', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    base = stubReferenceInvoke({
      list_accounts: mockAccounts,
      list_categories: [],
      list_insurers: [],
      list_merchants: [],
    })
  })

  it('初始化状态：账户/标的/数量/价格为空（数量/价格为原始文本，#416）', () => {
    const form = useInvestmentForm('buy')
    expect(form.accountId.value).toBeNull()
    expect(form.instrumentId.value).toBeNull()
    expect(form.quantityText.value).toBe('')
    expect(form.priceText.value).toBe('')
  })

  it('submit 校验：无账户/标的/数量/单价时警告且不写入', async () => {
    const form = useInvestmentForm('buy')
    await form.submit()
    form.accountId.value = 'acc-inv'
    await form.submit()
    form.instrumentId.value = 'ins-1'
    await form.submit()
    // 格式类错误（数量为空）由红态＋禁用接住，静默中止（ADR-0058）
    await form.submit()
    form.quantityText.value = '10'
    await form.submit()
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction'),
    ).toHaveLength(0)
  })

  it('submit 创建：调用 create_transaction，成功后重置表单', async () => {
    mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'create_transaction' ? Promise.resolve('new-txn') : base(cmd, args),
    )
    const onCreated = vi.fn()
    const form = useInvestmentForm('buy', { onCreated })
    form.accountId.value = 'acc-inv'
    form.instrumentId.value = 'ins-1'
    form.quantityText.value = '10'
    form.priceText.value = '15'
    form.fee.value = 5
    form.note.value = '测试'
    form.date.value = new Date('2026-07-11').getTime()

    await form.submit()

    // 提交路由：创建命令 + 正确 kind；wire 字段形状（含 buy 占位语义）由装配器
    // 测试承担（issue #216）
    expect(mockInvoke).toHaveBeenCalledWith('create_transaction', {
      input: expect.objectContaining({ kind: 'buy' }),
    })
    // 创建成功后重置业务字段（数量/价格文本同清、时机标志同清不留潜伏红态，#416）
    expect(form.instrumentId.value).toBeNull()
    expect(form.quantityText.value).toBe('')
    expect(form.priceText.value).toBe('')
    expect(form.quantityError.value).toBeNull()
    expect(onCreated).toHaveBeenCalledTimes(1)
  })

  describe('基金申赎形态（issue #302）：金额权威、单价反算', () => {
    /** 远程搜索填充基金候选（防抖 300ms，仿本文件 fake-timer 惯例） */
    async function searchFundCandidates(kind: 'buy' | 'sell') {
      vi.useFakeTimers()
      try {
        mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) =>
          cmd === 'list_instruments'
            ? Promise.resolve({ items: mockFundInstruments, total: 1 })
            : base(cmd, args),
        )
        const form = useInvestmentForm(kind)
        form.searchInstruments('某混合')
        await vi.advanceTimersByTimeAsync(300)
        return form
      } finally {
        vi.useRealTimers()
      }
    }

    it('选基金标的：isFundInstrument 打开，derivedPrice 按（金额 − 费用）× 100 ÷ 份额反算', async () => {
      const emptyForm = useInvestmentForm('buy')
      expect(emptyForm.isFundInstrument.value).toBe(false)
      expect(emptyForm.derivedPrice.value).toBeNull()
      const form = await searchFundCandidates('buy')
      form.instrumentId.value = 'ins-fund'
      expect(form.isFundInstrument.value).toBe(true)
      // (100000 − 150) × 100 ÷ 987.6543 = 10109.81… → 10110 → 1.0110 元
      form.amount.value = 1000
      form.quantityText.value = '987.6543'
      form.fee.value = 1.5
      expect(form.derivedPrice.value).toBeCloseTo(1.011, 6)
    })

    it('缺确认金额：警告且不写入（份额/单价校验不误伤）', async () => {
      const form = await searchFundCandidates('buy')
      form.instrumentId.value = 'ins-fund'
      form.accountId.value = 'acc-inv'
      form.quantityText.value = '987.6543'
      await form.submit()
      expect(
        mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction'),
      ).toHaveLength(0)
    })

    it('submit 创建：确认单金额落 amount_cents、单价不落 wire（price_cents null）', async () => {
      const form = await searchFundCandidates('buy')
      mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) =>
        cmd === 'create_transaction' ? Promise.resolve('fund-txn') : base(cmd, args),
      )
      form.instrumentId.value = 'ins-fund'
      form.accountId.value = 'acc-inv'
      form.amount.value = 1000
      form.quantityText.value = '987.6543'
      form.fee.value = 1.5
      await form.submit()
      expect(mockInvoke).toHaveBeenCalledWith('create_transaction', {
        input: expect.objectContaining({
          kind: 'buy',
          amount_cents: 100000,
          quantity: 987.6543,
          price_cents: null,
          fee_cents: 150,
        }),
      })
    })

    it('sell 反算口径：毛收入 = 金额 + 费用，derivedPrice 随之抬高', async () => {
      const form = await searchFundCandidates('sell')
      form.instrumentId.value = 'ins-fund'
      form.amount.value = 520
      form.quantityText.value = '500'
      form.fee.value = 0.52
      // (52000 + 52) × 100 ÷ 500 = 10410.4 → 10410 → 1.0410 元
      expect(form.derivedPrice.value).toBeCloseTo(1.041, 6)
    })

    it('编辑回填：确认单金额回填 amount，单价不回填（由反算展示）', () => {
      const form = useInvestmentForm('buy', {
        editing: () => editingFundTx,
        trade: () => editingFundTrade,
      })
      expect(form.isFundInstrument.value).toBe(true)
      expect(form.amount.value).toBe(1000)
      expect(form.quantityText.value).toBe('987.6543')
      expect(form.fee.value).toBe(1.5)
      expect(form.priceText.value).toBe('')
      // 基金形态无单价输入面，单价错误态不装配（#416）
      expect(form.priceError.value).toBeNull()
      // 反算展示与存储净值同一公式：(100000 − 150) × 100 ÷ 987.6543 → 1.0110 元
      expect(form.derivedPrice.value).toBeCloseTo(1.011, 6)
    })
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
      expect(form.quantityText.value).toBe('100')
      expect(form.priceText.value).toBe('150')
      // 合法回填不显红态（#416）
      expect(form.quantityError.value).toBeNull()
      expect(form.priceError.value).toBeNull()
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
      mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) =>
        cmd === 'list_instruments'
          ? Promise.resolve({ items: mockInstruments, total: 1 })
          : base(cmd, args),
      )
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
      mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) =>
        cmd === 'update_transaction' ? Promise.resolve(null) : base(cmd, args),
      )
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
      expect(form.quantityText.value).toBe('100')
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
      mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) =>
        cmd === 'update_transaction'
          ? Promise.reject(new Error('该买入交易已有部分卖出，无法修改'))
          : base(cmd, args),
      )

      await expect(form.submit()).resolves.toBeUndefined()
      expect(onUpdated).not.toHaveBeenCalled()
      expect(form.instrumentId.value).toBe('ins-1')
      expect(form.quantityText.value).toBe('100')
      expect(form.priceText.value).toBe('150')
    })
  })
})
