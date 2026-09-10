import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { messageCalls } from './helpers/message-mock'
import { useConvertForm } from '@/composables/useConvertForm'
import type { Account, Instrument, Transaction, TransactionConvert } from '@/types'

const mockAccounts: Account[] = [
  {
    id: 'acc-inv', name: '基金户', type: 'investment', currency_code: 'CNY',
    initial_balance_cents: 0, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false, is_hidden: false,
  },
]

const mockInstruments: Instrument[] = [
  {
    id: 'ins-out', symbol: '006793', name: '转出基金', type: 'fund', currency_code: 'CNY',
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false, market: 'unknown', invested: false,
    source: 'eastmoney', price_cents: null,
  },
]

const editingTx: Transaction = {
  id: 'txn-cv-1',
  kind: 'convert',
  amount_cents: 100000,
  currency_code: 'CNY',
  amount_native_cents: 100000,
  account_id: 'acc-inv',
  to_account_id: null,
  funding_account_id: null,
  category_id: null,
  merchant_id: null,
  policy_id: null,
  source: null,
  convert: null,
  refund_of_transaction_id: null,
  note: '换仓',
  date: '2026-02-01',
  created_at: '2026-02-01T01:00:00Z',
  updated_at: '2026-02-01T01:00:00Z',
  version: 1,
  device_id: 'test',
  is_deleted: false,
}

const editingConvert: TransactionConvert = {
  out_instrument_id: 'ins-out',
  out_symbol: '006793',
  out_instrument_name: '转出基金',
  out_quantity: 100.5,
  out_amount_cents: 110_550,
  in_instrument_id: 'ins-in',
  in_symbol: '519700',
  in_instrument_name: '转入基金',
  in_quantity: 99.75,
  in_amount_cents: 109_725,
  fee_cents: 150,
  carried_cost_cents: 100_000,
  currency_code: 'CNY',
}

/** 参考账户须为投资类型（acc-inv「基金户」），其余参考命令走兜底夹具。 */
const BASE_OVERRIDES = { list_accounts: mockAccounts }

describe('useConvertForm', () => {
  beforeEach(() => {
    wireInvokeSeam({ overrides: BASE_OVERRIDES })
  })

  it('初始化状态：账户/两腿标的/份额/金额为空', () => {
    const form = useConvertForm()
    expect(form.accountId.value).toBeNull()
    expect(form.outInstrumentId.value).toBeNull()
    expect(form.inInstrumentId.value).toBeNull()
    expect(form.outQuantityText.value).toBe('')
    expect(form.inQuantityText.value).toBe('')
    expect(form.outAmount.value).toBeNull()
    expect(form.inAmount.value).toBeNull()
  })

  it('submit 校验：缺账户/标的/金额或两标的相同均警告且不写入', async () => {
    const form = useConvertForm()
    await form.submit()
    form.accountId.value = 'acc-inv'
    await form.submit()
    form.outInstrumentId.value = 'ins-out'
    form.inInstrumentId.value = 'ins-in'
    await form.submit()
    form.outQuantityText.value = '10'
    form.inQuantityText.value = '10'
    await form.submit()
    form.outAmount.value = 110
    await form.submit()
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction'),
    ).toHaveLength(0)

    // 两标的相同：业务警告，不写入
    const same = useConvertForm()
    same.accountId.value = 'acc-inv'
    same.outInstrumentId.value = 'ins-out'
    same.inInstrumentId.value = 'ins-out'
    same.outQuantityText.value = '10'
    same.inQuantityText.value = '10'
    same.outAmount.value = 110
    same.inAmount.value = 110
    await same.submit()
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction'),
    ).toHaveLength(0)
    expect(messageCalls().some((c) => c.text === '转出标的与转入标的不能相同')).toBe(true)
  })

  it('反算单价：两侧各自按确认金额 ÷ 份额（万分之一元单次舍入），输入不完整为 null', () => {
    const form = useConvertForm()
    expect(form.derivedOutPrice.value).toBeNull()
    expect(form.derivedInPrice.value).toBeNull()
    form.outQuantityText.value = '100'
    form.outAmount.value = 110
    expect(form.derivedOutPrice.value).toBeCloseTo(1.1, 6)
    form.inQuantityText.value = '99'
    form.inAmount.value = 108.9
    expect(form.derivedInPrice.value).toBeCloseTo(1.1, 6)
  })

  it('submit 创建：create_transaction 携转换两腿 wire 形状，成功后重置表单', async () => {
    wireInvokeSeam({
      overrides: { ...BASE_OVERRIDES, create_transaction: Promise.resolve('new-txn') },
    })
    const onCreated = vi.fn()
    const form = useConvertForm({ onCreated })
    form.accountId.value = 'acc-inv'
    form.outInstrumentId.value = 'ins-out'
    form.inInstrumentId.value = 'ins-in'
    form.outQuantityText.value = '100.5'
    form.outAmount.value = 110.55
    form.inQuantityText.value = '99.75'
    form.inAmount.value = 109.72
    form.fee.value = 1.5
    form.note.value = '换仓'

    await form.submit()

    expect(mockInvoke).toHaveBeenCalledWith('create_transaction', {
      input: expect.objectContaining({
        kind: 'convert',
        amount_cents: 0,
        account_id: 'acc-inv',
        instrument_id: 'ins-out',
        quantity: 100.5,
        to_instrument_id: 'ins-in',
        to_quantity: 99.75,
        out_amount_cents: 11055,
        in_amount_cents: 10972,
        fee_cents: 150,
        // 转换不跨账户/不带出资账户：显式 null 占位（全字段替换不丢字段）
        to_account_id: null,
        funding_account_id: null,
      }),
    })
    expect(form.outInstrumentId.value).toBeNull()
    expect(form.inInstrumentId.value).toBeNull()
    expect(form.outQuantityText.value).toBe('')
    expect(onCreated).toHaveBeenCalledTimes(1)
  })

  it('远程搜索：转出侧候选按「代码 · 名称」呈现（防抖）', async () => {
    vi.useFakeTimers()
    try {
      wireInvokeSeam({
        overrides: { ...BASE_OVERRIDES, list_instruments: Promise.resolve({ items: mockInstruments, total: 1 }) },
      })
      const form = useConvertForm()
      form.searchOutInstruments('转出')
      await vi.advanceTimersByTimeAsync(300)
      expect(form.outInstrumentOptions.value).toEqual([{ label: '006793 · 转出基金', value: 'ins-out' }])
      // 转入侧独立候选，未搜索前为空
      expect(form.inInstrumentOptions.value).toEqual([])
    } finally {
      vi.useRealTimers()
    }
  })

  describe('编辑模式（issue #979）', () => {
    it('创建即回填：两腿标的/份额/确认金额/手续费/结转成本/备注/日期', () => {
      const form = useConvertForm({
        editing: () => editingTx,
        convert: () => editingConvert,
      })
      expect(form.accountId.value).toBe('acc-inv')
      expect(form.outInstrumentId.value).toBe('ins-out')
      expect(form.inInstrumentId.value).toBe('ins-in')
      expect(form.outQuantityText.value).toBe('100.5')
      expect(form.inQuantityText.value).toBe('99.75')
      expect(form.outAmount.value).toBe(1105.5)
      expect(form.inAmount.value).toBe(1097.25)
      expect(form.fee.value).toBe(1.5)
      expect(form.carriedCost.value).toBe(1000)
      expect(form.note.value).toBe('换仓')
      expect(form.date.value).toBe(new Date('2026-02-01T00:00:00Z').getTime())
      // 合法回填不显红态
      expect(form.outQuantityError.value).toBeNull()
      expect(form.inQuantityError.value).toBeNull()
      // 回填标的合入候选（显示 symbol · name，不裸 id）
      expect(form.outInstrumentOptions.value).toEqual([{ label: '006793 · 转出基金', value: 'ins-out' }])
      expect(form.inInstrumentOptions.value).toEqual([{ label: '519700 · 转入基金', value: 'ins-in' }])
    })

    it('提交走 update_transaction（全字段替换），不重置表单', async () => {
      wireInvokeSeam({
        overrides: { ...BASE_OVERRIDES, update_transaction: Promise.resolve(editingTx) },
      })
      const onUpdated = vi.fn()
      const form = useConvertForm({
        onUpdated,
        editing: () => editingTx,
        convert: () => editingConvert,
      })
      form.outQuantityText.value = '50'
      await form.submit()
      expect(mockInvoke).toHaveBeenCalledWith('update_transaction', {
        id: 'txn-cv-1',
        input: expect.objectContaining({ kind: 'convert', quantity: 50 }),
      })
      expect(onUpdated).toHaveBeenCalledTimes(1)
      // 编辑路径不重置：弹窗由父层关闭
      expect(form.outQuantityText.value).toBe('50')
    })
  })
})
