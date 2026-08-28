import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import { useCategoryForm } from '@/composables/useCategoryForm'
import type { Account, Category, Currency, Transaction } from '@/types'

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
    const store = useReferenceStore()
    await store.ensureFresh()
    const expenseForm = useCategoryForm('expense')
    expect(expenseForm.treeOptions.value).toHaveLength(1)
    expect(expenseForm.treeOptions.value[0].key).toBe('cat-1')

    const incomeForm = useCategoryForm('income')
    expect(incomeForm.treeOptions.value).toHaveLength(1)
    expect(incomeForm.treeOptions.value[0].key).toBe('cat-2')
  })

  it('accountOptions 来自 store', async () => {
    const store = useReferenceStore()
    await store.ensureFresh()
    const form = useCategoryForm('expense')
    expect(form.accountOptions.value).toHaveLength(1)
    expect(form.accountOptions.value[0]).toEqual({ label: '现金', value: 'acc-1' })
  })

  it('submit 校验：无账户时提示警告（不抛出）', async () => {
    const form = useCategoryForm('expense')
    // 不设 accountId，submit 应返回不抛异常
    await expect(form.submit()).resolves.toBeUndefined()
    // self-init 已自动加载参考数据，故按命令过滤断言无记账写入
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction'),
    ).toHaveLength(0)
  })

  it('submit 校验：金额为空时提示警告', async () => {
    const form = useCategoryForm('expense')
    form.accountId.value = 'acc-1'
    form.amount.value = null
    await expect(form.submit()).resolves.toBeUndefined()
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction'),
    ).toHaveLength(0)
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

    // self-init 已自动加载参考数据（list_*），此处仅断言记账写入恰一次
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction'),
    ).toHaveLength(1)
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

  describe('编辑模式（issue #178）', () => {
    const editingTx: Transaction = {
      id: 'txn-001',
      kind: 'expense',
      amount_cents: 12500,
      currency_code: 'CNY',
      amount_native_cents: 12500,
      account_id: 'acc-1',
      to_account_id: null,
      category_id: 'cat-1',
      refund_of_transaction_id: null,
      note: '原备注',
      date: '2026-02-10',
      created_at: '2026-02-01T00:00:00Z',
      updated_at: '2026-02-01T00:00:00Z',
      version: 1,
      device_id: 'test',
      is_deleted: false,
    }

    it('创建时按既有交易回填全部业务字段（金额按币种小数位换算、日期可回显）', () => {
      const form = useCategoryForm('expense', { editing: () => editingTx })
      expect(form.amount.value).toBe(125)
      expect(form.currencyCode.value).toBe('CNY')
      expect(form.accountId.value).toBe('acc-1')
      expect(form.categoryId.value).toBe('cat-1')
      expect(form.note.value).toBe('原备注')
      // 日期以 UTC 午夜回填（与提交端 toISOString 切片同一口径，往返无损）
      expect(new Date(form.date.value).toISOString().slice(0, 10)).toBe('2026-02-10')
    })

    it('submit 走更新命令：同形入参（无幂等键）+ 交易 id，成功触发 onUpdated 不触发 onCreated', async () => {
      mockInvoke.mockResolvedValue(undefined)
      const onCreated = vi.fn()
      const onUpdated = vi.fn()
      const form = useCategoryForm('expense', {
        onCreated,
        onUpdated,
        editing: () => editingTx,
      })
      // 用户修改金额/备注/日期
      form.amount.value = 90
      form.note.value = '修改'
      form.date.value = new Date('2026-02-15T00:00:00Z').getTime()

      await form.submit()

      const updateCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'update_transaction')
      expect(updateCalls).toHaveLength(1)
      expect(mockInvoke).toHaveBeenCalledWith('update_transaction', {
        id: 'txn-001',
        input: {
          kind: 'expense',
          amount_cents: 9000,
          currency_code: 'CNY',
          account_id: 'acc-1',
          category_id: 'cat-1',
          note: '修改',
          date: '2026-02-15',
        },
      })
      // 创建路径不分发
      expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')).toHaveLength(0)
      expect(onUpdated).toHaveBeenCalledTimes(1)
      expect(onCreated).not.toHaveBeenCalled()
    })

    it('编辑提交成功后不重置表单（弹窗由父层关闭，实例整体销毁）', async () => {
      mockInvoke.mockResolvedValue(undefined)
      const form = useCategoryForm('expense', { editing: () => editingTx })

      await form.submit()

      expect(form.amount.value).toBe(125)
      expect(form.note.value).toBe('原备注')
    })

    it('提交失败显示错误且不重置已填内容（弹窗保持打开，可修正重试）', async () => {
      mockInvoke.mockRejectedValue('金额必须大于 0')
      const onUpdated = vi.fn()
      const form = useCategoryForm('expense', { onUpdated, editing: () => editingTx })
      form.note.value = '改了一半'

      await expect(form.submit()).resolves.toBeUndefined()

      expect(onUpdated).not.toHaveBeenCalled()
      expect(form.amount.value).toBe(125)
      expect(form.note.value).toBe('改了一半')
    })
  })
})
