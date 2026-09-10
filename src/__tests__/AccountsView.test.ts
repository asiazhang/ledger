import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { wireInvokeSeam } from './helpers/invoke-mock'
import { fireProp } from './helpers/component-vm'
import { mount, flushPromises } from '@vue/test-utils'
import { NDataTable, NDialogProvider, NDropdown, NForm, NInput, NModal } from 'naive-ui'
import { setFakeMedia } from './helpers/media-mock'
import { h, nextTick } from 'vue'
import AccountsView from '@/views/AccountsView.vue'
import AccountLink from '@/components/AccountLink.vue'
import { amountPrivacyEnabled, formatAmount } from '@/utils/money'
import type { Account, AccountBalance } from '@/types'


const pushMock = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push: pushMock }),
}))

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
  pushMock.mockReset()
  // list_accounts 参考命令本场景需自定义值（acc-2「银行」，overrides 优先于参考兑底）；
  // 参考 store 预载走接缝 opt-in 参数。
  await wireInvokeSeam({
    defaults: { list_account_balances: mockBalances },
    overrides: { list_accounts: mockBalances.map((b) => b.account) },
    refreshReferenceStores: true,
  }).ready
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
    // NDropdown onSelect 装配缝（fireProp 单点窄化）：分派到编辑弹窗
    fireProp(rowMenu(wrapper), 'onSelect', 'edit')
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

describe('AccountsView 移动档（issue #847 / ADR-0088 决策 11 票⑦，词汇表「窗口分级」）', () => {
  /** 视图顶层调用 useDialog（删除二次确认），与 App.vue 同构需 NDialogProvider 包裹。 */
  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    })
  }

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

  /** 行「⋯」按钮（aria-label 随界面语言）。 */
  function moreButtons(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('button[aria-label="更多操作"]')
  }

  afterEach(() => {
    amountPrivacyEnabled.value = false
  })

  /** 本 describe 专用夹具：第二行为 bank 类型，副行「银行卡 · CNY」与名称
   * 「银行」可区分（验证类型/币种确实并入副行而非丢失）。 */
  const mobileBalances: AccountBalance[] = [
    { account: makeAccount('acc-1', '现金'), balance_cents: 1000 },
    { account: { ...makeAccount('acc-2', '银行'), type: 'bank' }, balance_cents: -500 },
  ]

  async function wireMobileBalances() {
    await wireInvokeSeam({
      defaults: { list_account_balances: mobileBalances },
      overrides: { list_accounts: mobileBalances.map((b) => b.account) },
    }).ready
  }

  it('桌面档零变化：五列全列、行内新增表单、「⋯」无 48px 扩径', async () => {
    const wrapper = mountView()
    await flushPromises()
    const table = wrapper.findComponent(NDataTable)
    expect((table.props('columns') as unknown[]).length).toBe(5)
    const createForm = wrapper.findAllComponents(NForm)[0]
    expect(createForm.props('inline')).toBe(true)
    const btn = moreButtons(wrapper)[0].element as HTMLElement
    expect(btn.style.width).toBe('')
  })

  it('移动档列结构三分：名称（类型/币种并入副行）、余额、操作——无横向滚动前提', async () => {
    setFakeMedia({ width: 600 })
    await wireMobileBalances()
    const wrapper = mountView()
    await flushPromises()
    const table = wrapper.findComponent(NDataTable)
    const columns = table.props('columns') as Array<{ key?: string }>
    expect(columns.map((c) => c.key)).toEqual(['account.name', 'balance_cents', 'actions'])
    // 类型与币种不丢：并入名称副行（第二行「银行」→ 银行卡 · CNY）
    expect(bodyRows(wrapper)[1].text()).toContain('银行卡 · CNY')
  })

  it('移动档：新增账户表单纵向堆叠（标签上置 + 控件满宽）', async () => {
    setFakeMedia({ width: 600 })
    const wrapper = mountView()
    await flushPromises()
    const createForm = wrapper.findAllComponents(NForm)[0]
    expect(createForm.props('inline')).toBe(false)
    expect(createForm.props('labelPlacement')).toBe('top')
  })

  it('移动档：「⋯」按钮 48px 触控目标（ADR-0088 全局验收基线，桌面不挂）', async () => {
    setFakeMedia({ width: 600 })
    const wrapper = mountView()
    await flushPromises()
    for (const btn of moreButtons(wrapper)) {
      const el = btn.element as HTMLElement
      expect(el.style.width).toBe('48px')
      expect(el.style.height).toBe('48px')
    }
  })

  it('移动档：账户名换行不截断（触屏无悬停全文，悬停替代「空间够则常驻」）；桌面保持 nowrap', async () => {
    setFakeMedia({ width: 600 })
    const mobile = mountView()
    await flushPromises()
    const mobileLink = mobile.findAllComponents(AccountLink)[0].element as HTMLElement
    expect(mobileLink.style.whiteSpace).toBe('normal')
    mobile.unmount()

    setFakeMedia({ width: 1280 })
    const desktop = mountView()
    await flushPromises()
    const desktopLink = desktop.findAllComponents(AccountLink)[0].element as HTMLElement
    expect(desktopLink.style.whiteSpace).toBe('')
  })

  it('跨断点缩窗实时换列（五列 ⇄ 三列）', async () => {
    const wrapper = mountView()
    await flushPromises()
    const table = wrapper.findComponent(NDataTable)
    expect((table.props('columns') as unknown[]).length).toBe(5)
    setFakeMedia({ width: 600 })
    await flushPromises()
    expect((table.props('columns') as unknown[]).length).toBe(3)
    setFakeMedia({ width: 1280 })
    await flushPromises()
    expect((table.props('columns') as unknown[]).length).toBe(5)
  })

  it('触控轴：「⋯」与右键两轴一致——同一菜单、同一选项闭集', async () => {
    setFakeMedia({ width: 600, hover: 'none', pointer: 'coarse' })
    const wrapper = mountView()
    await flushPromises()
    const expected = ['edit', 'adjust-balance', 'menu-divider', 'delete']

    // 触控轴无右键手势可达性要求，但两轴行为一致是验收硬条件：右键入口结果…
    await bodyRows(wrapper)[0].trigger('contextmenu')
    await flushPromises()
    expect(rowMenu(wrapper).props('show')).toBe(true)
    expect(menuKeys(wrapper)).toEqual(expected)
    // …点外部关闭（非模态家族既有通道）后，「⋯」入口得同一闭集
    fireProp(rowMenu(wrapper), 'onClickoutside')
    await flushPromises()
    expect(rowMenu(wrapper).props('show')).toBe(false)
    await moreButtons(wrapper)[1].trigger('click')
    await flushPromises()
    expect(rowMenu(wrapper).props('show')).toBe(true)
    expect(menuKeys(wrapper)).toEqual(expected)
  })

  it('移动档：金额隐私模式生效（余额掩码，名称/类型/币种副行不掩）', async () => {
    setFakeMedia({ width: 600 })
    await wireMobileBalances()
    const wrapper = mountView()
    await flushPromises()
    const visible = formatAmount(1000, { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 })
    expect(wrapper.text()).toContain(visible)
    amountPrivacyEnabled.value = true
    await nextTick()
    expect(wrapper.text()).toContain('••••')
    expect(wrapper.text()).not.toContain(visible)
    // 副行不是金额：类型与币种照常可读
    expect(bodyRows(wrapper)[1].text()).toContain('银行卡 · CNY')
    amountPrivacyEnabled.value = false
    await nextTick()
    expect(wrapper.text()).toContain(visible)
  })
})
