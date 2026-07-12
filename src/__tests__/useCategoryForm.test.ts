import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '@/stores/app'
import { useCategoryForm } from '@/composables/useCategoryForm'
import type { Account, Category, Currency } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1', name: '现金', type: 'cash', currency_code: 'CNY',
    initial_balance_cents: 0, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
]

const mockCategories: Category[] = [
  {
    id: 'cat-1', name: '餐饮', kind: 'expense', parent_id: null,
    icon: null, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
  {
    id: 'cat-2', name: '工资', kind: 'income', parent_id: null,
    icon: null, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
]

describe('useCategoryForm', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve(mockCategories)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
  })

  it('初始化状态：金额为空、币种CNY、账户为空', () => {
    const form = useCategoryForm('expense')
    expect(form.amount.value).toBeNull()
    expect(form.currencyCode.value).toBe('CNY')
    expect(form.accountId.value).toBeNull()
    expect(form.categoryId.value).toBeNull()
    expect(form.note.value).toBe('')
  })

  it('treeOptions 根据 kind 过滤', async () => {
    const store = useAppStore()
    await store.loadAll()
    const expenseForm = useCategoryForm('expense')
    expect(expenseForm.treeOptions.value).toHaveLength(1)
    expect(expenseForm.treeOptions.value[0].key).toBe('cat-1')

    const incomeForm = useCategoryForm('income')
    expect(incomeForm.treeOptions.value).toHaveLength(1)
    expect(incomeForm.treeOptions.value[0].key).toBe('cat-2')
  })

  it('accountOptions 来自 store', async () => {
    const store = useAppStore()
    await store.loadAll()
    const form = useCategoryForm('expense')
    expect(form.accountOptions.value).toHaveLength(1)
    expect(form.accountOptions.value[0]).toEqual({ label: '现金', value: 'acc-1' })
  })

  it('submit 校验：无账户时提示警告（不抛出）', async () => {
    const form = useCategoryForm('expense')
    // 不设 accountId，submit 应返回不抛异常
    await expect(form.submit()).resolves.toBeUndefined()
    expect(mockInvoke).not.toHaveBeenCalled()
  })

  it('submit 校验：金额为空时提示警告', async () => {
    const form = useCategoryForm('expense')
    form.accountId.value = 'acc-1'
    form.amount.value = null
    await expect(form.submit()).resolves.toBeUndefined()
    expect(mockInvoke).not.toHaveBeenCalled()
  })

  it('submit 调用 api.createTransaction', async () => {
    mockInvoke.mockResolvedValue('new-txn-id')
    const form = useCategoryForm('expense')
    form.accountId.value = 'acc-1'
    form.amount.value = 100
    form.categoryId.value = 'cat-1'
    form.note.value = '午餐'
    form.date.value = new Date('2026-07-11').getTime()

    await form.submit()

    expect(mockInvoke).toHaveBeenCalledTimes(1)
    expect(mockInvoke).toHaveBeenCalledWith('create_transaction', {
      input: {
        kind: 'expense',
        amount_cents: 10000,
        currency_code: 'CNY',
        account_id: 'acc-1',
        category_id: 'cat-1',
        note: '午餐',
        date: '2026-07-11',
      },
    })
  })

  it('submit 成功后重置表单', async () => {
    mockInvoke.mockResolvedValue('new-txn-id')
    const form = useCategoryForm('expense')
    form.accountId.value = 'acc-1'
    form.amount.value = 50
    form.note.value = '测试'

    await form.submit()

    expect(form.amount.value).toBeNull()
    expect(form.note.value).toBe('')
  })

  it('submit 失败时 catch 错误不抛出', async () => {
    mockInvoke.mockRejectedValue(new Error('网络错误'))
    const form = useCategoryForm('expense')
    form.accountId.value = 'acc-1'
    form.amount.value = 100

    await expect(form.submit()).resolves.toBeUndefined()
  })

  it('resetForm 恢复初始状态', () => {
    const form = useCategoryForm('expense')
    form.amount.value = 100
    form.note.value = 'test'
    form.accountId.value = 'acc-1'
    form.categoryId.value = 'cat-1'

    form.resetForm()

    expect(form.amount.value).toBeNull()
    expect(form.note.value).toBe('')
    expect(form.accountId.value).toBeNull()
    expect(form.categoryId.value).toBeNull()
    expect(form.currencyCode.value).toBe('CNY')
  })

  it('onCreated 回调在 submit 成功后触发', async () => {
    mockInvoke.mockResolvedValue('new-txn-id')
    const onCreated = vi.fn()
    const form = useCategoryForm('expense', { onCreated })
    form.accountId.value = 'acc-1'
    form.amount.value = 100

    await form.submit()

    expect(onCreated).toHaveBeenCalledTimes(1)
  })
})
