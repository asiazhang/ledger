import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { NSelect, NInputNumber, NDatePicker } from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import BudgetView from '@/views/BudgetView.vue'
import type { Category, Currency } from '@/types'

const mockInvoke = vi.mocked(invoke)

enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

// 覆写 setup.ts 的 useMessage mock：改用稳定实例以便断言反馈分支
// （spec：提交失败→把后端错误清晰呈现给用户）。
const messageApi = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
  info: vi.fn(),
  loading: vi.fn(),
  destroyAll: vi.fn(),
}))
vi.mock('naive-ui', async (importOriginal) => {
  const actual = await importOriginal<typeof import('naive-ui')>()
  return { ...actual, useMessage: () => messageApi }
})

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockCategories: Category[] = [
  {
    id: 'cat-1',
    name: '餐饮',
    kind: 'expense',
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 'cat-2',
    name: '工资',
    kind: 'income',
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
  },
]

/** 挂载视图（参考数据经 store ensureFresh 注入），flush 后就绪。 */
async function mountView() {
  const wrapper = mount(BudgetView)
  await flushPromises()
  return wrapper
}

/** 从分类下拉（收窄为支出分类选项）选中指定分类。 */
function pickCategory(wrapper: Awaited<ReturnType<typeof mountView>>, id: string) {
  wrapper.findComponent(NSelect).vm.$emit('update:value', id)
}

/** 填金额与起始月并点击「添加」。起始月固定 2026-07（UTC 时间戳，toISOString 口径为 '2026-07-01'）。 */
async function submitAmount(wrapper: Awaited<ReturnType<typeof mountView>>, amount: number) {
  wrapper.findComponent(NInputNumber).vm.$emit('update:value', amount)
  wrapper.findComponent(NDatePicker).vm.$emit('update:value', Date.UTC(2026, 6, 1))
  const add = wrapper.findAll('button').find((b) => b.text() === '添加')
  expect(add, '应存在「添加」按钮').toBeDefined()
  await add!.trigger('click')
  await flushPromises()
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  messageApi.success.mockReset()
  messageApi.warning.mockReset()
  messageApi.error.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve(mockCategories)
    if (cmd === 'list_merchants') return Promise.resolve([])
    if (cmd === 'budget_progress') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  const store = useReferenceStore()
  await store.ensureFresh()
})

describe('BudgetView 预算表单（issue #183）', () => {
  it('分类下拉只提供支出分类（收入分类不可选）', async () => {
    const wrapper = await mountView()
    const options = wrapper.findComponent(NSelect).props('options') as {
      label: string
      value: string
    }[]
    expect(options).toEqual([{ label: '餐饮', value: 'cat-1' }])
  })

  it('金额非正前置拦截，不发起后端调用', async () => {
    const wrapper = await mountView()
    pickCategory(wrapper, 'cat-1')
    await submitAmount(wrapper, 0)
    expect(messageApi.warning).toHaveBeenCalledWith('预算金额必须为正数')
    expect(mockInvoke).not.toHaveBeenCalledWith('create_budget', expect.anything())
  })

  it('提交成功清空表单并提示', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'create_budget') return Promise.resolve('budget-1')
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve([])
      if (cmd === 'list_categories') return Promise.resolve(mockCategories)
      if (cmd === 'list_merchants') return Promise.resolve([])
      if (cmd === 'budget_progress') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = await mountView()
    pickCategory(wrapper, 'cat-1')
    await submitAmount(wrapper, 500)
    expect(mockInvoke).toHaveBeenCalledWith('create_budget', {
      input: {
        category_id: 'cat-1',
        amount_cents: 50000,
        start_date: '2026-07-01',
      },
    })
    expect(messageApi.success).toHaveBeenCalledWith('已创建预算')
  })

  it('提交失败把后端中文错误清晰呈现（AppError 序列化形态）', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'create_budget') {
        return Promise.reject({ kind: 'Invalid', message: '该分类已存在按月预算' })
      }
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve([])
      if (cmd === 'list_categories') return Promise.resolve(mockCategories)
      if (cmd === 'list_merchants') return Promise.resolve([])
      if (cmd === 'budget_progress') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = await mountView()
    pickCategory(wrapper, 'cat-1')
    await submitAmount(wrapper, 100)
    expect(messageApi.error).toHaveBeenCalledWith('创建失败: 该分类已存在按月预算')
  })
})
