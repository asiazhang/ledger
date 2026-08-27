import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import DashboardView from '@/views/DashboardView.vue'
import TransactionForm from '@/components/TransactionForm.vue'
import { useReferenceStore } from '@/stores/reference'
import type { Account, AccountBalance, Currency } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1',
    name: '现金',
    type: 'cash',
    currency_code: 'CNY',
    initial_balance_cents: 10000,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    is_hidden: false,
  },
]

const mockBalances: AccountBalance[] = [
  { account: mockAccounts[0], balance_cents: 10000 },
]

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_account_balances') return Promise.resolve(mockBalances)
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  localStorage.clear()
  const store = useReferenceStore()
  await store.ensureFresh()
})

async function mountView() {
  const wrapper = mount(DashboardView)
  await flushPromises()
  return wrapper
}

describe('DashboardView 快速记账与最近交易移除（issue #141）', () => {
  it('不再渲染快速记账表单（TransactionForm）', async () => {
    const wrapper = await mountView()
    expect(wrapper.findComponent(TransactionForm).exists()).toBe(false)
    expect(wrapper.text()).not.toContain('快速记账')
  })

  it('不再渲染最近交易列表', async () => {
    const wrapper = await mountView()
    expect(wrapper.text()).not.toContain('最近交易')
    // 不再查询交易列表
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'list_transactions')).toBe(false)
  })

  it('账户余额卡片保留（仪表盘改造前的既有内容）', async () => {
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('现金')
    expect(wrapper.text()).toContain('¥100.00')
  })
})
