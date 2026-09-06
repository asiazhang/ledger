import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import { resolveLendingDirection } from '@/domain/lending'
import { useLendingForm } from '@/composables/useLendingForm'
import type { Account, Currency, Transaction } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

/** 覆盖资金侧（cash/bank）、借出侧（receivable）、负债侧（debt）的账户集 */
const mockAccounts: Account[] = [
  acc('acc-cash', '现金', 'cash'),
  acc('acc-bank', '银行', 'bank'),
  acc('acc-recv-zhang', '借出·张三', 'receivable'),
  acc('acc-recv-co', '借出·XX公司', 'receivable'),
  acc('acc-debt-li', '借入·李四', 'debt'),
]

function acc(id: string, name: string, type: Account['type']): Account {
  return {
    id,
    name,
    type,
    currency_code: 'CNY',
    initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    is_hidden: false,
  }
}

function optionIds(options: Array<{ value: string }>) {
  return options.map((o) => o.value)
}

const FUND_IDS = ['acc-cash', 'acc-bank']
const RECV_IDS = ['acc-recv-zhang', 'acc-recv-co']
const DEBT_IDS = ['acc-debt-li']

describe('useLendingForm（借贷变体 composable，issue #374 S3）', () => {
  beforeEach(async () => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_insurers') return Promise.resolve([])
      if (cmd === 'list_merchants') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    // 账户过滤集断言依赖参考数据就绪：等 self-init 拉取完成
    await useReferenceStore().refresh()
  })

  describe('账户过滤集随方向切换', () => {
    it('默认预置「借出」：转出=资金账户、转入=receivable 账户', () => {
      const form = useLendingForm()
      expect(form.direction.value).toBe('lend')
      expect(optionIds(form.fromAccountOptions.value)).toEqual(FUND_IDS)
      expect(optionIds(form.toAccountOptions.value)).toEqual(RECV_IDS)
    })

    it('「借入」入口预置：转出=debt 账户、转入=资金账户', () => {
      const form = useLendingForm({ initialDirection: 'borrow' })
      expect(form.direction.value).toBe('borrow')
      expect(optionIds(form.fromAccountOptions.value)).toEqual(DEBT_IDS)
      expect(optionIds(form.toAccountOptions.value)).toEqual(FUND_IDS)
    })

    it('方向 toggle 覆盖四个方向：每方向转出/转入侧别与过滤表一致', () => {
      const form = useLendingForm()
      form.setDirection('collect')
      expect(optionIds(form.fromAccountOptions.value)).toEqual(RECV_IDS)
      expect(optionIds(form.toAccountOptions.value)).toEqual(FUND_IDS)
      form.setDirection('borrow')
      expect(optionIds(form.fromAccountOptions.value)).toEqual(DEBT_IDS)
      expect(optionIds(form.toAccountOptions.value)).toEqual(FUND_IDS)
      form.setDirection('repay')
      expect(optionIds(form.fromAccountOptions.value)).toEqual(FUND_IDS)
      expect(optionIds(form.toAccountOptions.value)).toEqual(DEBT_IDS)
    })
  })

  describe('方向切换时的已选账户处置', () => {
    it('反向方向（借出↔收回）：交换两端，不丢已选账户', () => {
      const form = useLendingForm()
      form.accountId.value = 'acc-cash'
      form.toAccountId.value = 'acc-recv-zhang'
      form.setDirection('collect')
      expect(form.direction.value).toBe('collect')
      expect(form.accountId.value).toBe('acc-recv-zhang')
      expect(form.toAccountId.value).toBe('acc-cash')
    })

    it('反向方向（借入↔还款）：交换两端', () => {
      const form = useLendingForm({ initialDirection: 'borrow' })
      form.accountId.value = 'acc-debt-li'
      form.toAccountId.value = 'acc-bank'
      form.setDirection('repay')
      expect(form.accountId.value).toBe('acc-bank')
      expect(form.toAccountId.value).toBe('acc-debt-li')
    })

    it('不可交换的方向切换：越侧的已选账户清空、仍合规的保留', () => {
      const form = useLendingForm()
      form.accountId.value = 'acc-cash'
      form.toAccountId.value = 'acc-recv-zhang'
      // 借出 → 还款：转出侧同为资金账户保留，转入侧 receivable 不再合规清空
      form.setDirection('repay')
      expect(form.accountId.value).toBe('acc-cash')
      expect(form.toAccountId.value).toBeNull()
    })

    it('借贷户互转的非法组合切走时：越侧清空、合规侧保留', () => {
      const form = useLendingForm()
      form.accountId.value = 'acc-recv-zhang'
      form.toAccountId.value = 'acc-debt-li'
      // 借出 → 还款：转入侧 debt 仍合规保留，转出侧 receivable 越侧清空
      form.setDirection('repay')
      expect(form.accountId.value).toBeNull()
      expect(form.toAccountId.value).toBe('acc-debt-li')
    })
  })

  describe('提交路由（与转账同构）', () => {
    it('借出提交调 create_transaction：kind=transfer、方向即双账户填法', async () => {
      mockInvoke.mockResolvedValue('new-txn-id')
      const form = useLendingForm()
      form.accountId.value = 'acc-cash'
      form.toAccountId.value = 'acc-recv-zhang'
      form.amountText.value = '1000'

      await form.submit()

      expect(mockInvoke).toHaveBeenCalledWith('create_transaction', {
        input: expect.objectContaining({
          kind: 'transfer',
          account_id: 'acc-cash',
          to_account_id: 'acc-recv-zhang',
        }),
      })
    })

    it('还款提交同样落 transfer（debt→资金）', async () => {
      mockInvoke.mockResolvedValue('new-txn-id')
      const form = useLendingForm({ initialDirection: 'borrow' })
      form.setDirection('repay')
      form.accountId.value = 'acc-bank'
      form.toAccountId.value = 'acc-debt-li'
      form.amountText.value = '200'

      await form.submit()

      expect(mockInvoke).toHaveBeenCalledWith('create_transaction', {
        input: expect.objectContaining({
          kind: 'transfer',
          account_id: 'acc-bank',
          to_account_id: 'acc-debt-li',
        }),
      })
    })

    it('语义校验复用转账：转出=转入拒绝写入', async () => {
      const form = useLendingForm()
      form.accountId.value = 'acc-cash'
      form.toAccountId.value = 'acc-cash'
      form.amountText.value = '100'

      await form.submit()

      expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')).toHaveLength(0)
    })
  })

  describe('编辑模式（借贷形态回填）', () => {
    /** 历史借贷：现金 → 借出·张三（transfer，账户类型正确即自动获得借贷形态） */
    const editingTx: Transaction = {
      id: 'txn-200',
      kind: 'transfer',
      amount_cents: 30000,
      currency_code: 'CNY',
      amount_native_cents: 30000,
      account_id: 'acc-cash',
      to_account_id: 'acc-recv-zhang',
      category_id: null,
      merchant_id: null,
      policy_id: null,
      refund_of_transaction_id: null,
      note: '借给张三',
      date: '2026-05-01',
      created_at: '2026-05-01T00:00:00Z',
      updated_at: '2026-05-01T00:00:00Z',
      version: 1,
      device_id: 'test',
      is_deleted: false,
    }

    it('按既有交易的账户类型派生方向并回填（含金额）', () => {
      const form = useLendingForm({ editing: () => editingTx })
      expect(form.direction.value).toBe('lend')
      expect(form.accountId.value).toBe('acc-cash')
      expect(form.toAccountId.value).toBe('acc-recv-zhang')
      // 金额按币种小数位换算回填（30000 分 → 300 元，不手写 /100）
      expect(form.amountText.value).toBe('300')
    })

    it('提交走更新命令、kind 恒 transfer（方向只影响双账户填法）', async () => {
      mockInvoke.mockResolvedValue(undefined)
      const onUpdated = vi.fn()
      const form = useLendingForm({ editing: () => editingTx, onUpdated })
      form.amountText.value = '500'

      await form.submit()

      expect(mockInvoke).toHaveBeenCalledWith('update_transaction', {
        id: 'txn-200',
        input: expect.objectContaining({
          kind: 'transfer',
          account_id: 'acc-cash',
          to_account_id: 'acc-recv-zhang',
        }),
      })
      expect(onUpdated).toHaveBeenCalledTimes(1)
    })

    it('账户类型缺失（未知账户）回退预置方向，不误判借贷', () => {
      const unknownTx = { ...editingTx, to_account_id: 'acc-gone' }
      const form = useLendingForm({ editing: () => unknownTx, initialDirection: 'borrow' })
      expect(form.direction.value).toBe('borrow')
    })
  })
})

describe('resolveLendingDirection（编辑形态识别，S1 消费点）', () => {
  const typeOf = (id: string) => mockAccounts.find((a) => a.id === id)?.type

  it('四个方向的转账识别为对应借贷方向', () => {
    expect(resolveLendingDirection(
      { kind: 'transfer', account_id: 'acc-cash', to_account_id: 'acc-recv-zhang' },
      typeOf,
    )).toBe('lend')
    expect(resolveLendingDirection(
      { kind: 'transfer', account_id: 'acc-recv-zhang', to_account_id: 'acc-cash' },
      typeOf,
    )).toBe('collect')
    expect(resolveLendingDirection(
      { kind: 'transfer', account_id: 'acc-debt-li', to_account_id: 'acc-cash' },
      typeOf,
    )).toBe('borrow')
    expect(resolveLendingDirection(
      { kind: 'transfer', account_id: 'acc-cash', to_account_id: 'acc-debt-li' },
      typeOf,
    )).toBe('repay')
  })

  it('普通转账 / 非 transfer → null（按普通转账呈现）；借贷侧 + 缺失对端 → 仍派借贷（issue #374 修订）', () => {
    expect(resolveLendingDirection(
      { kind: 'transfer', account_id: 'acc-cash', to_account_id: 'acc-bank' },
      typeOf,
    )).toBeNull()
    expect(resolveLendingDirection(
      { kind: 'expense', account_id: 'acc-cash', to_account_id: null },
      typeOf,
    )).toBeNull()
    // 借贷侧 + 对端账户取不到类型（黑洞 is_hidden / 已删 / 不可查）→ 仍按借贷方向派生：
    // 未知转出 → receivable = 借出，不因对端缺失退回普通转账。
    expect(resolveLendingDirection(
      { kind: 'transfer', account_id: 'acc-gone', to_account_id: 'acc-recv-zhang' },
      typeOf,
    )).toBe('lend')
  })
})
