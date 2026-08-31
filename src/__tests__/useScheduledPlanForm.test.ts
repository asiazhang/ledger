import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useScheduledPlanForm } from '@/composables/useScheduledPlanForm'
import { useReferenceStore } from '@/stores/reference'
import { useAppStore } from '@/stores/app'
import { todayStr } from '@/utils/date'
import type { Account, Category, Currency, Merchant } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1',
    name: '招商银行',
    type: 'cash',
    currency_code: 'CNY',
    initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    is_hidden: false,
  },
]

const mockCategories: Category[] = [
  {
    id: 'cat-1',
    name: '订阅服务',
    kind: 'expense',
    parent_id: null,
    icon: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

const mockMerchants: Merchant[] = [
  {
    id: 'mch-1',
    name: '视频平台',
    icon: null,
    color: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

function mockBaseCommands(merchants: Merchant[] = mockMerchants) {
  mockInvoke.mockImplementation(((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve(mockCategories)
    if (cmd === 'list_merchants') return Promise.resolve(merchants)
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  }) as typeof invoke)
}

/** 商户解析结果：填入草稿商户值后直接调接缝解析（不 mount 组件）。 */
async function resolvedMerchant(merchantValue: string | null): Promise<string | null> {
  await useReferenceStore().refresh()
  const form = useScheduledPlanForm()
  form.merchantRef.value = merchantValue
  return form.resolveMerchant()
}

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockBaseCommands()
})

describe('useScheduledPlanForm 商户解析（输入即建 + 重名兜底，ADR-0041）', () => {
  it('不填商户：null，不调用 create_merchant', async () => {
    const id = await resolvedMerchant(null)
    expect(id).toBeNull()
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_merchant')).toBe(false)
  })

  it('选中已有商户（value 为 id）：原样携带，不调用 create_merchant', async () => {
    const id = await resolvedMerchant('mch-1')
    expect(id).toBe('mch-1')
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_merchant')).toBe(false)
  })

  it('输入在用商户名（未选 id）：按名复用，不调用 create_merchant', async () => {
    const id = await resolvedMerchant('视频平台')
    expect(id).toBe('mch-1')
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_merchant')).toBe(false)
  })

  it('输入空白字符：null（不即建空名商户）', async () => {
    const id = await resolvedMerchant('   ')
    expect(id).toBeNull()
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_merchant')).toBe(false)
  })

  it('输入新名字（未命中）：保存即建商户并返回新 id', async () => {
    mockInvoke.mockImplementation(((cmd: string) => {
      if (cmd === 'create_merchant') return Promise.resolve('mch-new')
      if (cmd === 'list_merchants') return Promise.resolve(mockMerchants)
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve(mockCategories)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    }) as typeof invoke)
    const id = await resolvedMerchant('盒马')
    expect(mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_merchant')).toEqual([
      'create_merchant',
      { input: { name: '盒马' } },
    ])
    expect(id).toBe('mch-new')
  })

  it('即建撞重名（store 陈旧竞态）：强制重拉后按名复用已有商户，不报错', async () => {
    let stale = true
    mockInvoke.mockImplementation(((cmd: string) => {
      if (cmd === 'create_merchant') {
        return Promise.reject(new Error('参数错误: 商户已存在: 盒马'))
      }
      if (cmd === 'list_merchants') {
        const rows: Merchant[] = stale
          ? mockMerchants
          : [
              ...mockMerchants,
              {
                id: 'mch-exist',
                name: '盒马',
                icon: null,
                color: null,
                created_at: '2026-01-01T00:00:00Z',
                updated_at: '2026-01-01T00:00:00Z',
                version: 1,
                device_id: 'test',
                is_deleted: false,
              },
            ]
        stale = false
        return Promise.resolve(rows)
      }
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve(mockCategories)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    }) as typeof invoke)
    const id = await resolvedMerchant('盒马')
    expect(id).toBe('mch-exist')
  })

  it('重名兜底后重拉仍无此名：原 create 错误上抛', async () => {
    mockInvoke.mockImplementation(((cmd: string) => {
      if (cmd === 'create_merchant') return Promise.reject(new Error('商户已存在'))
      if (cmd === 'list_merchants') return Promise.resolve(mockMerchants)
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve(mockCategories)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    }) as typeof invoke)
    await expect(resolvedMerchant('盒马')).rejects.toThrow('商户已存在')
  })

  it('重名兜底中重拉失败：仍上抛原 create 错误', async () => {
    await useReferenceStore().refresh() // 首拉成功（陈旧字典）
    let pulled = false
    mockInvoke.mockImplementation(((cmd: string) => {
      if (cmd === 'create_merchant') return Promise.reject(new Error('商户已存在'))
      if (cmd === 'list_merchants') {
        // 首拉已成功，之后的兜底重拉一律失败：吞掉重拉错误，保留原错误
        if (pulled) return Promise.reject(new Error('重拉失败'))
        pulled = true
        return Promise.resolve(mockMerchants)
      }
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve(mockCategories)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    }) as typeof invoke)
    const form = useScheduledPlanForm()
    form.merchantRef.value = '盒马'
    await expect(form.resolveMerchant()).rejects.toThrow('商户已存在')
  })

  describe('编辑路径（订阅编辑弹窗，编辑中商户 id 参数）', () => {
    /** 空字典 + 指定编辑中商户 id 建表单：模拟原商户软删且超出会话缓存。 */
    async function resolvedEditing(
      merchantValue: string | null,
      editingMerchantId: string | null,
    ): Promise<string | null> {
      mockBaseCommands([])
      await useReferenceStore().refresh()
      const form = useScheduledPlanForm()
      form.merchantRef.value = merchantValue
      return form.resolveMerchant(editingMerchantId)
    }

    it('未改动原商户（软删不在字典）：传编辑中商户 id 原样携带（软删兜底分支）', async () => {
      const id = await resolvedEditing('mch-gone', 'mch-gone')
      expect(id).toBe('mch-gone')
      expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_merchant')).toBe(false)
    })

    it('同值但不传编辑中商户 id：不再原样携带，走按名解析（兜底以参数为准）', async () => {
      mockInvoke.mockImplementation(((cmd: string) => {
        if (cmd === 'create_merchant') return Promise.resolve('mch-new')
        if (cmd === 'list_merchants') return Promise.resolve([])
        if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
        if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
        if (cmd === 'list_categories') return Promise.resolve(mockCategories)
        return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
      }) as typeof invoke)
      await useReferenceStore().refresh()
      const form = useScheduledPlanForm()
      form.merchantRef.value = 'mch-gone'
      const id = await form.resolveMerchant(null)
      expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_merchant')).toBe(true)
      expect(id).toBe('mch-new')
    })
  })
})

describe('useScheduledPlanForm 公共 payload 组装（CreateScheduledInput 公共字段单源）', () => {
  it('订阅形态：公共字段单源组装，备注 trim，不带形态特化键', async () => {
    await useReferenceStore().refresh()
    const form = useScheduledPlanForm()
    form.note.value = '  视频会员  '
    form.accountId.value = 'acc-1'
    form.categoryId.value = 'cat-1'
    form.recurrenceInterval.value = 3
    form.startDate.value = '2026-02-15'
    expect(form.buildCreateInput({ kind: 'subscription', amountCents: 2500, merchantId: 'mch-1' })).toEqual({
      kind: 'subscription',
      account_id: 'acc-1',
      category_id: 'cat-1',
      merchant_id: 'mch-1',
      amount_cents: 2500,
      currency_code: 'CNY',
      recurrence_type: 'monthly',
      recurrence_interval: 3,
      recurrence_day: null,
      start_date: '2026-02-15',
      note: '视频会员',
    })
  })

  it('形态特化字段由页签携带：仅透传页签给出的键，不补空键', async () => {
    await useReferenceStore().refresh()
    const form = useScheduledPlanForm()
    const installment = form.buildCreateInput({
      kind: 'installment',
      amountCents: 8333,
      merchantId: null,
      specific: { total_amount_cents: 100000, total_occurrences: 12 },
    })
    expect(installment).toMatchObject({ total_amount_cents: 100000, total_occurrences: 12 })
    expect('to_account_id' in installment).toBe(false)

    const transfer = form.buildCreateInput({
      kind: 'scheduled_transfer',
      amountCents: 50000,
      merchantId: null,
      specific: { to_account_id: 'acc-2', total_occurrences: null },
    })
    expect(transfer).toMatchObject({ to_account_id: 'acc-2', total_occurrences: null })
    expect('total_amount_cents' in transfer).toBe(false)
  })

  it('纯空白备注组装为 null（三形态统一「空 → 无备注」口径）', async () => {
    await useReferenceStore().refresh()
    const form = useScheduledPlanForm()
    form.note.value = '   '
    form.accountId.value = 'acc-1'
    const input = form.buildCreateInput({ kind: 'subscription', amountCents: 100, merchantId: null })
    expect(input.note).toBeNull()
  })
})

describe('useScheduledPlanForm 草稿初始态与重置', () => {
  it('初始草稿：备注空、账户/分类/商户空、币种=默认币种、周期每月×1、开始日=今天', () => {
    const form = useScheduledPlanForm()
    expect(form.note.value).toBe('')
    expect(form.accountId.value).toBeNull()
    expect(form.categoryId.value).toBeNull()
    expect(form.merchantRef.value).toBeNull()
    expect(form.currencyCode.value).toBe(useAppStore().defaultCurrency)
    expect(form.recurrenceType.value).toBe('monthly')
    expect(form.recurrenceInterval.value).toBe(1)
    expect(form.recurrenceDay.value).toBeNull()
    expect(form.startDate.value).toBe(todayStr())
  })

  it('reset 回初始态：模态语义下每次打开是全新表单（币种回默认币种）', () => {
    const form = useScheduledPlanForm()
    form.note.value = '音乐订阅'
    form.accountId.value = 'acc-1'
    form.categoryId.value = 'cat-1'
    form.merchantRef.value = '盒马'
    form.currencyCode.value = 'USD'
    form.recurrenceType.value = 'weekly'
    form.recurrenceInterval.value = 2
    form.recurrenceDay.value = 5
    form.startDate.value = '2026-03-01'
    form.reset()
    expect(form.note.value).toBe('')
    expect(form.accountId.value).toBeNull()
    expect(form.categoryId.value).toBeNull()
    expect(form.merchantRef.value).toBeNull()
    expect(form.currencyCode.value).toBe(useAppStore().defaultCurrency)
    expect(form.recurrenceType.value).toBe('monthly')
    expect(form.recurrenceInterval.value).toBe(1)
    expect(form.recurrenceDay.value).toBeNull()
    expect(form.startDate.value).toBe(todayStr())
  })

  it('工厂形态：两次调用独立实例，互不串扰', () => {
    const create = useScheduledPlanForm()
    const edit = useScheduledPlanForm()
    create.note.value = '新建草稿'
    expect(edit.note.value).toBe('')
  })
})

describe('useScheduledPlanForm 选项面（参考数据单源）', () => {
  it('账户/币种/支出分类树/在用商户选项来自参考数据', async () => {
    await useReferenceStore().refresh()
    const form = useScheduledPlanForm()
    expect(form.accountOptions.value.map((o) => o.value)).toEqual(['acc-1'])
    expect(form.currencyOptions.value.map((o) => o.value)).toEqual(['CNY'])
    expect(form.merchantOptions.value).toEqual([{ label: '视频平台', value: 'mch-1' }])
    // 分类树只含支出类（分期/订阅扣款为支出口径）
    expect(JSON.stringify(form.categoryTreeOptions.value)).toContain('订阅服务')
  })
})
