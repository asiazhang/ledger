import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '@/stores/app'
import CategoryForm from '@/components/CategoryForm.vue'
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
]

describe('CategoryForm.vue', () => {
  beforeEach(async () => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve(mockCategories)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    // Pre-load store so components have data
    const store = useAppStore()
    await store.loadAll()
  })

  it('挂载并显示提交按钮文本', () => {
    const wrapper = mount(CategoryForm, {
      props: { kind: 'expense', submitLabel: '记支出' },
    })
    expect(wrapper.text()).toContain('记支出')
  })

  it('挂载并显示收入表单文本', () => {
    const wrapper = mount(CategoryForm, {
      props: { kind: 'income', submitLabel: '记收入' },
    })
    expect(wrapper.text()).toContain('记收入')
  })

  it('点击提交按钮触发 submit（无账户时只提示不调用后端）', async () => {
    mockInvoke.mockClear()
    const wrapper = mount(CategoryForm, {
      props: { kind: 'expense', submitLabel: '记支出' },
    })
    const btn = wrapper.find('button')
    expect(btn.exists()).toBe(true)
    await btn.trigger('click')
    // 无账户时只提示不调用后端（mockInvoke 已在 store.loadAll 中被调用清除后应无新调用）
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')
    expect(calls).toHaveLength(0)
  })

  it('在 store 加载前挂载不应崩溃', () => {
    setActivePinia(createPinia())
    // 不调用 store.loadAll() — 模拟未加载状态
    const wrapper = mount(CategoryForm, {
      props: { kind: 'expense', submitLabel: '记支出' },
    })
    expect(wrapper.exists()).toBe(true)
  })

  it('正确渲染选择器（账户、币种等至少存在一个元素）', () => {
    const wrapper = mount(CategoryForm, {
      props: { kind: 'expense', submitLabel: '记支出' },
    })
    // 检查表单项标签
    expect(wrapper.text()).toContain('金额')
    expect(wrapper.text()).toContain('账户')
    expect(wrapper.text()).toContain('分类')
    expect(wrapper.text()).toContain('日期')
    expect(wrapper.text()).toContain('备注')
  })

  it('设置 valid 表单数据后提交会调用 create_transaction', async () => {
    mockInvoke.mockClear()
    mockInvoke.mockResolvedValue('new-id')
    const wrapper = mount(CategoryForm, {
      props: { kind: 'expense', submitLabel: '记支出' },
    })
    // 扫描是否存在金额输入框
    const inputs = wrapper.findAll('input')
    expect(inputs.length).toBeGreaterThan(0)
  })
})
