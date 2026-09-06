import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NDialogProvider, NDropdown, NForm, NInput, NModal } from 'naive-ui'
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
    if (cmd === 'list_insurers') return Promise.resolve([])
    if (cmd === 'list_account_balances') return Promise.resolve(mockBalances)
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  localStorage.clear()
  const store = useReferenceStore()
  await store.refresh()
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

describe('AccountsView 行菜单冒烟（issue #551：右键 + 「⋯」双入口）', () => {
  /** 表格数据行。 */
  function bodyRows(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('.n-data-table-tbody .n-data-table-tr')
  }

  /** 行菜单：视图内唯一 NDropdown（按 options 含 edit key 识别）。 */
  function rowMenu(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAllComponents(NDropdown).find((d) =>
      (d.props('options') as Array<{ key?: string }>).some((o) => o.key === 'edit'),
    )!
  }

  function menuKeys(wrapper: ReturnType<typeof mount>) {
    return (rowMenu(wrapper).props('options') as Array<{ key: string }>).map((o) => o.key)
  }

  /** 右键指定行打开菜单。 */
  async function openMenuOnRow(wrapper: ReturnType<typeof mount>, index = 0) {
    await bodyRows(wrapper)[index].trigger('contextmenu')
    await flushPromises()
  }

  /** 点击指定行操作列「⋯」按钮打开菜单（第二入口，aria-label 随界面语言）。 */
  async function openMenuOnMoreButton(wrapper: ReturnType<typeof mount>, index = 0) {
    await wrapper.findAll('button[aria-label="更多操作"]')[index].trigger('click')
    await flushPromises()
  }

  /** 视图顶层调用 useDialog（删除二次确认），与 App.vue 同构需 NDialogProvider 包裹。 */
  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    })
  }

  it('行右键弹出行菜单：编辑 / 调整余额 / 删除', async () => {
    const wrapper = mountView()
    await flushPromises()
    expect(rowMenu(wrapper).props('show')).toBe(false)
    await openMenuOnRow(wrapper, 0)
    expect(rowMenu(wrapper).props('show')).toBe(true)
    expect(menuKeys(wrapper)).toEqual(['edit', 'adjust-balance', 'menu-divider', 'delete'])
  })

  it('操作列「⋯」按钮弹出行菜单（第二入口，与右键共用同一菜单）', async () => {
    const wrapper = mountView()
    await flushPromises()
    await openMenuOnMoreButton(wrapper, 1)
    expect(rowMenu(wrapper).props('show')).toBe(true)
    expect(menuKeys(wrapper)).toEqual(['edit', 'adjust-balance', 'menu-divider', 'delete'])
  })

  it('菜单选中「编辑」分派到编辑弹窗并回填目标行（第二行「银行」）', async () => {
    const wrapper = mountView()
    await flushPromises()
    await openMenuOnRow(wrapper, 1)
    rowMenu(wrapper).props('onSelect')?.('edit')
    await flushPromises()
    const editModal = wrapper
      .findAllComponents(NModal)
      .find((m) => m.props('title') === '编辑账户')!
    expect(editModal.props('show')).toBe(true)
    // 回填目标行：编辑弹窗表单（全局第 2 个 NForm，第 1 个为顶部新增表单）内
    // 首个 NInput 即名称字段，值为右键目标行的账户名——分派到的确是收起菜单
    // 瞬间的目标行
    const editForm = wrapper.findAllComponents(NForm)[1]
    expect(editForm.findComponent(NInput).props('value')).toBe('银行')
  })
})
