import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { setActivePinia, createPinia } from 'pinia'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import { useTransferForm } from '@/composables/useTransferForm'
import type { Account, Transaction } from '@/types'


const mockAccounts: Account[] = [
  {
    id: 'acc-1', name: '现金', type: 'cash', currency_code: 'CNY',
    initial_balance_cents: 0, created_at: '2026-01-01T00:00:00Z', is_hidden: false,
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
  {
    id: 'acc-2', name: '银行', type: 'bank', currency_code: 'CNY',
    initial_balance_cents: 0, created_at: '2026-01-01T00:00:00Z', is_hidden: false,
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
]

describe('useTransferForm', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    // 参考命令桩统一走共享助手（issue #725）：币种与规范夹具等值流入，账户保留本文件夹具
    stubReferenceInvoke({
      list_accounts: mockAccounts,
      list_categories: [],
      list_insurers: [],
      list_merchants: [],
    })
  })

  it('submit 校验：转出转入账户相同时提示警告（不调用写入命令）', async () => {
    const form = useTransferForm()
    form.accountId.value = 'acc-1'
    form.toAccountId.value = 'acc-1'
    form.amountText.value = '100'

    await expect(form.submit()).resolves.toBeUndefined()
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')).toHaveLength(0)
  })

  it('submit 调用 api.createTransaction（创建路径）', async () => {
    mockInvoke.mockResolvedValue('new-txn-id')
    const form = useTransferForm()
    form.accountId.value = 'acc-1'
    form.toAccountId.value = 'acc-2'
    form.amountText.value = '200'

    await form.submit()

    // 提交路由：创建命令 + 正确 kind；wire 字段形状由装配器测试承担（issue #216）
    expect(mockInvoke).toHaveBeenCalledWith('create_transaction', {
      input: expect.objectContaining({ kind: 'transfer' }),
    })
  })

  describe('编辑模式（issue #178）', () => {
    const editingTx: Transaction = {
      id: 'txn-100',
      kind: 'transfer',
      amount_cents: 50000,
      currency_code: 'CNY',
      amount_native_cents: 50000,
      account_id: 'acc-1',
      to_account_id: 'acc-2',
      category_id: null,
      merchant_id: null,
      policy_id: null,
      source: null,
      refund_of_transaction_id: null,
      note: '房租',
      date: '2026-03-01',
      created_at: '2026-03-01T00:00:00Z',
      updated_at: '2026-03-01T00:00:00Z',
      version: 3,
      device_id: 'test',
      is_deleted: false,
    }

    it('创建时按既有交易回填全部业务字段', () => {
      const form = useTransferForm({ editing: () => editingTx })
      expect(form.amountText.value).toBe('500')
      expect(form.currencyCode.value).toBe('CNY')
      expect(form.accountId.value).toBe('acc-1')
      expect(form.toAccountId.value).toBe('acc-2')
      expect(form.note.value).toBe('房租')
      expect(new Date(form.date.value).toISOString().slice(0, 10)).toBe('2026-03-01')
    })

    it('submit 走更新命令：同形入参（无幂等键）+ 交易 id，成功触发 onUpdated', async () => {
      mockInvoke.mockResolvedValue(undefined)
      const onCreated = vi.fn()
      const onUpdated = vi.fn()
      const form = useTransferForm({ onCreated, onUpdated, editing: () => editingTx })
      // 用户改转入账户与金额
      form.toAccountId.value = 'acc-1'
      form.accountId.value = 'acc-2'
      form.amountText.value = '300'

      await form.submit()

      // 提交路由：更新命令携带交易 id + 用户改动字段交接装配结果；
      // 金额/日期转换与占位字段由装配器测试承担（issue #216）
      expect(mockInvoke).toHaveBeenCalledWith('update_transaction', {
        id: 'txn-100',
        input: expect.objectContaining({
          kind: 'transfer',
          account_id: 'acc-2',
          to_account_id: 'acc-1',
          note: '房租',
        }),
      })
      expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')).toHaveLength(0)
      expect(onUpdated).toHaveBeenCalledTimes(1)
      expect(onCreated).not.toHaveBeenCalled()
    })

    it('提交失败不重置已填内容且不触发 onUpdated', async () => {
      mockInvoke.mockRejectedValue('转账必须指定目标账户')
      const onUpdated = vi.fn()
      const form = useTransferForm({ onUpdated, editing: () => editingTx })
      form.toAccountId.value = null

      await expect(form.submit()).resolves.toBeUndefined()

      expect(onUpdated).not.toHaveBeenCalled()
      expect(form.amountText.value).toBe('500')
      expect(form.note.value).toBe('房租')
    })
  })
})
