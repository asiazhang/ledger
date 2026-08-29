import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NDialogProvider } from 'naive-ui'
import { h } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import AccountsView from '@/views/AccountsView.vue'
import AccountLink from '@/components/AccountLink.vue'
import type { Account, AccountBalance, Currency } from '@/types'

const mockInvoke = vi.mocked(invoke)

const pushMock = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push: pushMock }),
}))

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

function makeAccount(id: string, name: string): Account {
  return {
    id,
    name,
    type: 'cash',
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

const mockBalances: AccountBalance[] = [
  { account: makeAccount('acc-1', '现金'), balance_cents: 1000 },
  { account: makeAccount('acc-2', '银行'), balance_cents: -500 },
]

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  pushMock.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockBalances.map((b) => b.account))
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
    if (cmd === 'list_account_balances') return Promise.resolve(mockBalances)
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  localStorage.clear()
  const store = useReferenceStore()
  await store.ensureFresh()
})

describe('AccountsView 账户名下钻（issue #97）', () => {
  it('账户名称渲染为可点击组件（标题提示查看该账户的交易）', async () => {
    const wrapper = mountView()
    await flushPromises()
    const links = wrapper.findAllComponents(AccountLink)
    expect(links.length).toBe(2)
    expect(links[0].text()).toBe('现金')
    expect(links[0].attributes('title')).toBe('查看该账户的交易')
  })

  it('点击账户名称跳转交易页并携带该账户过滤参数', async () => {
    const wrapper = mountView()
    await flushPromises()
    const links = wrapper.findAllComponents(AccountLink)
    await links[1].find('button').trigger('click')
    expect(pushMock).toHaveBeenCalledWith({
      name: 'transactions',
      query: { account: 'acc-2' },
    })
  })

  /** 视图顶层调用 useDialog（删除二次确认），与 App.vue 同构需 NDialogProvider 包裹。 */
  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    })
  }
})
