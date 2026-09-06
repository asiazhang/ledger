import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useCategoryForm } from '@/composables/useCategoryForm'
import { useReferenceStore } from '@/stores/reference'
import { usePoliciesStore } from '@/stores/policies'
import type { Account, Currency, Merchant, Policy, Transaction } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1', name: '现金', type: 'cash', currency_code: 'CNY',
    initial_balance_cents: 0, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false, is_hidden: false,
  },
]

const mockMerchants: Merchant[] = [
  {
    id: 'mch-1', name: '京东',
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
]

const mockPolicies: Policy[] = [
  {
    id: 'pol-1', merchant_id: 'mch-1', policy_number: 'P2026-001',
    product_name: '重疾险', start_date: '2026-01-01', end_date: '2036-01-01',
    coverage_amount_cents: null, coverage_currency_code: null,
    note: null, is_deleted: false, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
  },
]

const editingTx: Transaction = {
  id: 'txn-1',
  kind: 'expense',
  amount_cents: 5000,
  currency_code: 'CNY',
  amount_native_cents: 5000,
  account_id: 'acc-1',
  to_account_id: null,
  category_id: null,
  merchant_id: 'mch-1',
  refund_of_transaction_id: null,
  note: null,
  date: '2026-02-01',
  created_at: '2026-02-01T00:00:00Z',
}

function mockBaseCommands(merchants: Merchant[] = mockMerchants) {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve(merchants)
    if (cmd === 'list_insurers') return Promise.resolve([])
    if (cmd === 'list_policies') return Promise.resolve(mockPolicies)
    if (cmd === 'list_policy_stats') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

function createCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')
}

function updateCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'update_transaction')
}

/** 提交入参：创建路径取 create_transaction，编辑路径取 update_transaction。 */
function submitCallInput(): Record<string, unknown> | undefined {
  const call = createCalls()[0] ?? updateCalls()[0]
  return (call?.[1] as { input: Record<string, unknown> } | undefined)?.input
}

function merchantCreateCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_merchant')
}

/** 填必填项后提交，返回 create_transaction 入参（不存在时为 undefined）。 */
async function submitWithMerchant(
  merchantRef: string | null,
  options?: Parameters<typeof useCategoryForm>[1],
) {
  // 参考 store self-init 异步：先等首拉完成，保证 merchantByName/merchantMap 就绪
  await useReferenceStore().refresh()
  await usePoliciesStore().refresh()
  const form = useCategoryForm('expense', options)
  form.amountText.value = '50'
  form.accountId.value = 'acc-1'
  form.merchantRef.value = merchantRef
  await form.submit()
  return submitCallInput()
}

describe('useCategoryForm 商户输入（issue #189）', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    mockBaseCommands()
  })

  it('不填商户：merchant_id 为 null，不调用 create_merchant', async () => {
    const input = await submitWithMerchant(null)
    expect(input?.merchant_id).toBeNull()
    expect(merchantCreateCalls()).toHaveLength(0)
  })

  it('选中已有商户（value 为 id）：直接携带，不调用 create_merchant', async () => {
    const input = await submitWithMerchant('mch-1')
    expect(input?.merchant_id).toBe('mch-1')
    expect(merchantCreateCalls()).toHaveLength(0)
  })

  it('输入已有商户名（未选 id）：按名解析复用，不调用 create_merchant', async () => {
    const input = await submitWithMerchant('京东')
    expect(input?.merchant_id).toBe('mch-1')
    expect(merchantCreateCalls()).toHaveLength(0)
  })

  it('输入新名字（未命中）：保存即建商户并携带新 id', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'create_merchant') return Promise.resolve('mch-new')
      if (cmd === 'list_merchants') return Promise.resolve(mockMerchants)
      if (cmd === 'list_insurers') return Promise.resolve([])
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_policies') return Promise.resolve(mockPolicies)
      if (cmd === 'list_policy_stats') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const input = await submitWithMerchant('盒马')
    expect(merchantCreateCalls()).toHaveLength(1)
    expect(merchantCreateCalls()[0]).toEqual(['create_merchant', { input: { name: '盒马' } }])
    expect(input?.merchant_id).toBe('mch-new')
  })

  it('即建撞重名（store 陈旧）：强制重拉后按名复用已有商户，不报错', async () => {
    let stale = true
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'create_merchant') {
        return Promise.reject(new Error('参数错误: 商户已存在: 盒马'))
      }
      if (cmd === 'list_merchants') {
        const rows: Merchant[] = stale
          ? mockMerchants
          : [
              ...mockMerchants,
              {
                id: 'mch-exist', name: '盒马',
                created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
                version: 1, device_id: 'test', is_deleted: false,
              },
            ]
        stale = false
        return Promise.resolve(rows)
      }
      if (cmd === 'list_insurers') return Promise.resolve([])
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_policies') return Promise.resolve(mockPolicies)
      if (cmd === 'list_policy_stats') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const input = await submitWithMerchant('盒马')
    expect(input?.merchant_id).toBe('mch-exist')
  })

  describe('编辑模式', () => {
    it('回填既有商户并原样提交（改名/软删均不影响 id 引用）', () => {
      const form = useCategoryForm('expense', { editing: () => editingTx })
      expect(form.merchantRef.value).toBe('mch-1')
    })

    it('编辑未动商户：提交保持原 merchant_id', async () => {
      const input = await submitWithMerchant('mch-1', { editing: () => editingTx })
      expect(input?.merchant_id).toBe('mch-1')
    })

    it('编辑时清除商户：merchant_id 为 null', async () => {
      const input = await submitWithMerchant(null, { editing: () => editingTx })
      expect(input?.merchant_id).toBeNull()
    })

    it('原商户已被软删（不在字典）：提交保持原 id（历史引用照常保留），兜底选项可显示', async () => {
      mockBaseCommands([]) // 字典为空：mch-1 已软删
      await useReferenceStore().refresh()
      const form = useCategoryForm('expense', { editing: () => editingTx })
      // 回填时不可用 uuid 裸值展示：兜底选项以可读标签承载原 id
      expect(form.merchantOptions.value.some((o) => o.value === 'mch-1')).toBe(true)
      form.amountText.value = '50'
      form.accountId.value = 'acc-1'
      await form.submit()
      const input = submitCallInput() as { merchant_id: string | null }
      expect(input.merchant_id).toBe('mch-1')
    })
  })

  describe('保单选择器（issue #361）', () => {
    /** 填必填项 + 保单后提交，返回 create_transaction 入参。 */
    async function submitWithPolicy(policyId: string | null) {
      await useReferenceStore().refresh()
      await usePoliciesStore().refresh()
      const form = useCategoryForm('expense')
      form.amountText.value = '50'
      form.accountId.value = 'acc-1'
      form.policyId.value = policyId
      await form.submit()
      return submitCallInput()
    }

    it('不选保单：policy_id 为 null（不挂单与现状一致）', async () => {
      const input = await submitWithPolicy(null)
      expect(input?.policy_id).toBeNull()
    })

    it('选中已有保单：policy_id 原样携带', async () => {
      const input = await submitWithPolicy('pol-1')
      expect(input?.policy_id).toBe('pol-1')
    })

    it('选项含未删除保单（保单号标签）', async () => {
      await usePoliciesStore().refresh()
      const form = useCategoryForm('expense')
      expect(form.policyOptions.value).toEqual([
        { label: 'P2026-001（重疾险）', value: 'pol-1' },
      ])
    })

    it('编辑回填原挂保单；原保单已软删（不在列表）时提交保持原 id（历史引用不置空），兜底选项可显示', async () => {
      const editingWithPolicy: Transaction = { ...editingTx, policy_id: 'pol-1' }
      const form = useCategoryForm('expense', { editing: () => editingWithPolicy })
      expect(form.policyId.value).toBe('pol-1')
      // 模拟原保单已软删：重拉后 store 列表不含 pol-1
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'list_policies') return Promise.resolve([])
        if (cmd === 'list_merchants') return Promise.resolve(mockMerchants)
        if (cmd === 'list_insurers') return Promise.resolve([])
        if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
        if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
        if (cmd === 'list_categories') return Promise.resolve([])
        return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
      })
      await usePoliciesStore().refresh()
      expect(form.policyOptions.value.some((o) => o.value === 'pol-1')).toBe(true)
      form.amountText.value = '50'
      form.accountId.value = 'acc-1'
      await form.submit()
      const input = submitCallInput() as { policy_id: string | null }
      expect(input.policy_id).toBe('pol-1')
    })
  })
})
