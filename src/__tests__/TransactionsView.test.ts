import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { reactive } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { NDataTable, NPopconfirm, NSelect, NDatePicker, NButton, NModal, NInputNumber } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import TransactionsView from '@/views/TransactionsView.vue'
import AccountLink from '@/components/AccountLink.vue'
import TransactionForm from '@/components/TransactionForm.vue'
import type { Account, Currency, Transaction } from '@/types'

const mockInvoke = vi.mocked(invoke)

// 路由 mock：TransactionsView 经 useRoute 读取 URL query（?account=<id> 只读入口）。
// 测试通过改写 routeMock.query 模拟带参/不带参进入与 query 变化；
// AccountLink 经 useRouter 跳转（pushMock 断言导航目标，issue #97/#99）。
const routeMock = reactive<{ query: Record<string, string | string[] | null> }>({ query: {} })
const pushMock = vi.fn()
vi.mock('vue-router', () => ({
  useRoute: () => routeMock,
  useRouter: () => ({ push: pushMock }),
}))

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1',
    name: '现金',
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
  {
    id: 'acc-2',
    name: '银行',
    type: 'bank',
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

function makeTxn(i: number, accountId = 'acc-1', overrides: Partial<Transaction> = {}): Transaction {
  return {
    id: `txn-${String(i).padStart(3, '0')}`,
    kind: 'expense',
    amount_cents: i * 100,
    currency_code: 'CNY',
    amount_native_cents: i * 100,
    account_id: accountId,
    to_account_id: null,
    category_id: null,
    refund_of_transaction_id: null,
    note: `备注 ${i}`,
    date: '2026-01-01',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...overrides,
  }
}

/** 可变的交易库：删除操作会真实移除，分页返回随 total 变化。
 * 偶数序号在 acc-2、奇数序号在 acc-1，供涉及账户过滤断言。 */
let txnDb: Transaction[] = []

/** 与后端 read.rs 口径一致：涉及账户 / 日期起止 / 类型 / 分页 AND 组合过滤。 */
function applyListFilter(filter: Record<string, unknown>) {
  return txnDb.filter((t) => {
    if (filter.involving_account_id) {
      const id = filter.involving_account_id as string
      if (t.account_id !== id && t.to_account_id !== id) return false
    }
    if (filter.from && t.date < (filter.from as string)) return false
    if (filter.to && t.date > (filter.to as string)) return false
    if (filter.kind && t.kind !== (filter.kind as string)) return false
    return true
  })
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  pushMock.mockReset()
  routeMock.query = {}
  txnDb = Array.from({ length: 45 }, (_, i) =>
    makeTxn(i + 1, i % 2 === 0 ? 'acc-2' : 'acc-1'),
  )
  mockInvoke.mockImplementation((cmd: string, args?: { filter?: Record<string, unknown>; id?: string }) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_transactions') {
      const filter = (args?.filter ?? {}) as Record<string, unknown>
      const scoped = applyListFilter(filter)
      const pageSize = (filter.page_size as number) ?? scoped.length
      const page = (filter.page as number) ?? 1
      const start = (page - 1) * pageSize
      return Promise.resolve({
        items: scoped.slice(start, start + pageSize),
        total: scoped.length,
      })
    }
    if (cmd === 'delete_transaction') {
      txnDb = txnDb.filter((t) => t.id !== args?.id)
      return Promise.resolve()
    }
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  localStorage.clear()
  const store = useReferenceStore()
  await store.ensureFresh()
})

async function mountView() {
  const wrapper = mount(TransactionsView)
  await flushPromises()
  return wrapper
}

function listCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_transactions')
}

function lastListFilter() {
  const calls = listCalls()
  const [, args] = calls[calls.length - 1] as [string, { filter: Record<string, unknown> }]
  return args.filter
}

function tablePagination(wrapper: ReturnType<typeof mount>) {
  return wrapper.findComponent(NDataTable).props('pagination') as {
    page: number
    pageSize: number
    itemCount: number
    onChange: (page: number) => void
    onUpdatePageSize: (pageSize: number) => void
  }
}

function bodyRows(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAll('.n-data-table-tbody .n-data-table-tr')
}

describe('TransactionsView 服务端分页', () => {
  it('默认以 page=1 page_size=20 查询并渲染「共 N 条」总数', async () => {
    const wrapper = await mountView()
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 20 })
    expect(wrapper.text()).toContain('共 45 条')
    // 只渲染当前页数据（不全量加载）
    expect(bodyRows(wrapper).length).toBe(20)
  })

  it('翻页以新的 page 重新查询', async () => {
    const wrapper = await mountView()
    const before = listCalls().length
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    expect(listCalls().length).toBe(before + 1)
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20 })
    expect(bodyRows(wrapper).length).toBe(20)
  })

  it('切换页大小以新的 page_size 查询并重置到第 1 页', async () => {
    const wrapper = await mountView()
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    tablePagination(wrapper).onUpdatePageSize(50)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 50 })
    expect(bodyRows(wrapper).length).toBe(45)
  })

  it('删除当前页一条后以当前页码刷新', async () => {
    const wrapper = await mountView()
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    const before = listCalls().length
    const popcons = wrapper.findAllComponents(NPopconfirm)
    await popcons[0].props('onPositiveClick')()
    await flushPromises()
    // 第 2 页原本 20 条，删 1 条不触发回退，仍刷新第 2 页
    expect(listCalls().length).toBe(before + 1)
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20 })
  })

  it('删除当前页最后一条后回退到上一页避免空页', async () => {
    const wrapper = await mountView()
    tablePagination(wrapper).onChange(3) // 第 3 页共 5 条
    await flushPromises()
    expect(bodyRows(wrapper).length).toBe(5)
    // 删除前 4 条（每删一条都会重新渲染，popconfirm 列表需重新获取）
    for (let i = 0; i < 4; i++) {
      const popcons = wrapper.findAllComponents(NPopconfirm)
      await popcons[0].props('onPositiveClick')()
      await flushPromises()
    }
    expect(bodyRows(wrapper).length).toBe(1)
    // 删除最后一条 → 自动回退到第 2 页，不出现空页
    const popcons = wrapper.findAllComponents(NPopconfirm)
    await popcons[0].props('onPositiveClick')()
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20 })
    expect(bodyRows(wrapper).length).toBe(20)
  })

  it('查询期间 loading 状态可见', async () => {
    let resolveList!: (v: unknown) => void
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_transactions') {
        return new Promise((resolve) => {
          resolveList = resolve
        })
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mount(TransactionsView)
    await flushPromises()
    // 参考数据已就绪（self-init），list_transactions 挂起中 → loading 应为 true
    expect(wrapper.findComponent(NDataTable).props('loading')).toBe(true)
    resolveList({ items: [], total: 0 })
    await flushPromises()
    expect(wrapper.findComponent(NDataTable).props('loading')).toBe(false)
  })
})

describe('TransactionsView 涉及账户 URL 过滤（issue #97）', () => {
  it('带有效 account 参数进入时自动按该账户过滤（含转入转账语义的参数）', async () => {
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    expect(lastListFilter()).toMatchObject({
      page: 1,
      page_size: 20,
      involving_account_id: 'acc-1',
    })
    // 45 笔中奇数序号（acc-1）共 22 笔（偶数序号在 acc-2）
    expect(wrapper.text()).toContain('共 22 条')
    expect(bodyRows(wrapper).length).toBe(20)
  })

  it('带无效 account 参数（账户不存在）进入时回退全量且不报错', async () => {
    routeMock.query = { account: 'missing-acc' }
    const wrapper = await mountView()
    expect(lastListFilter()).not.toHaveProperty('involving_account_id')
    expect(wrapper.text()).toContain('共 45 条')
  })

  it('不带 account 参数进入时复位为全量列表', async () => {
    routeMock.query = {}
    const wrapper = await mountView()
    expect(lastListFilter()).not.toHaveProperty('involving_account_id')
    expect(wrapper.text()).toContain('共 45 条')
  })

  it('已挂载时清除 account 参数复位为全量并回到第 1 页', async () => {
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    expect(lastListFilter()).toMatchObject({ involving_account_id: 'acc-1', page: 1 })
    // 先翻到第 2 页
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2, involving_account_id: 'acc-1' })
    // 导航清除 query（如从侧边栏重新进入交易页）→ 复位全量 + 回第 1 页
    routeMock.query = {}
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 1 })
    expect(lastListFilter()).not.toHaveProperty('involving_account_id')
    expect(wrapper.text()).toContain('共 45 条')
  })

  it('冷启动直连深链：参考数据晚到时有效 account 参数仍被应用（不静默丢失）', async () => {
    // 全新 pinia：参考数据尚未加载（self-init 在途），立即以带参 URL 挂载
    setActivePinia(createPinia())
    routeMock.query = { account: 'acc-1' }
    const wrapper = mount(TransactionsView)
    await flushPromises()
    // 参考数据就绪后自动补判：过滤被应用，而非永久回退全量
    expect(lastListFilter()).toMatchObject({ involving_account_id: 'acc-1' })
    expect(wrapper.text()).toContain('共 22 条')
  })
})

describe('TransactionsView 手动过滤（issue #98）', () => {
  // 富数据集：不同账户/日期/类型，供单条件、组合、空态断言（每 describe 前置重置）。
  const richDb: Transaction[] = [
    makeTxn(1, 'acc-1', { kind: 'expense', date: '2026-01-05' }),
    makeTxn(2, 'acc-2', { kind: 'income', date: '2026-02-10' }),
    makeTxn(3, 'acc-1', { kind: 'transfer', date: '2026-03-15', to_account_id: 'acc-2' }),
    makeTxn(4, 'acc-2', { kind: 'expense', date: '2026-01-20' }),
    makeTxn(5, 'acc-1', { kind: 'refund', date: '2026-02-25' }),
  ]

  beforeEach(() => {
    txnDb = [...richDb]
  })

  // 过滤行控件定位：账户下拉 = 第 1 个 NSelect，类型下拉 = 第 2 个
  const accountSelect = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NSelect)[0]
  const kindSelect = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NSelect)[1]
  const datePickers = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NDatePicker)
  const clearButton = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NButton).find((b) => b.text().includes('清除筛选'))!

  /** 直接向过滤行控件 emit 变更事件（与 SearchView.test 的 setDate 模式一致）。 */
  async function setAccount(wrapper: ReturnType<typeof mount>, id: string | null) {
    accountSelect(wrapper).vm.$emit('update:value', id)
    await flushPromises()
  }
  async function setKind(wrapper: ReturnType<typeof mount>, k: string | null) {
    kindSelect(wrapper).vm.$emit('update:value', k)
    await flushPromises()
  }
  async function setDateFrom(wrapper: ReturnType<typeof mount>, v: string | null) {
    datePickers(wrapper)[0].vm.$emit('update:formattedValue', v)
    await flushPromises()
  }
  async function setDateTo(wrapper: ReturnType<typeof mount>, v: string | null) {
    datePickers(wrapper)[1].vm.$emit('update:formattedValue', v)
    await flushPromises()
  }

  it('顶部渲染过滤行：账户/类型下拉可清除、起止日期、清除筛选按钮', async () => {
    const wrapper = await mountView()
    // 账户下拉：可清除，选项来自参考数据账户映射
    const account = accountSelect(wrapper)
    expect(account.props('clearable')).toBe(true)
    expect(
      (account.props('options') as { value: string; label: string }[]).map((o) => o.value),
    ).toEqual(['acc-1', 'acc-2'])
    // 日期起止
    expect(datePickers(wrapper).length).toBe(2)
    // 类型下拉：可清除，6 种交易类型（income/expense/transfer/refund/buy/sell）
    const kind = kindSelect(wrapper)
    expect(kind.props('clearable')).toBe(true)
    expect((kind.props('options') as { value: string }[]).map((o) => o.value)).toEqual([
      'income',
      'expense',
      'transfer',
      'refund',
      'buy',
      'sell',
    ])
    // 清除筛选按钮：无过滤时禁用
    expect(clearButton(wrapper).attributes('disabled')).toBeDefined()
  })

  it('选择账户即重新查询：involving_account_id 正确传后端（含转账转入侧）', async () => {
    const wrapper = await mountView()
    const before = listCalls().length
    await setAccount(wrapper, 'acc-2')
    expect(listCalls().length).toBe(before + 1)
    expect(lastListFilter()).toMatchObject({
      page: 1,
      page_size: 20,
      involving_account_id: 'acc-2',
    })
    // 涉及 acc-2：income(txn-2)、expense(txn-4)、transfer 转入侧(txn-3)
    expect(wrapper.text()).toContain('共 3 条')
  })

  it('选择日期范围即重新查询：from/to 正确传后端（含边界）', async () => {
    const wrapper = await mountView()
    await setDateFrom(wrapper, '2026-02-01')
    expect(lastListFilter()).toMatchObject({ from: '2026-02-01' })
    expect(wrapper.text()).toContain('共 3 条') // txn-2 (02-10) / txn-3 (03-15) / txn-5 (02-25)
    await setDateTo(wrapper, '2026-02-20')
    expect(lastListFilter()).toMatchObject({ from: '2026-02-01', to: '2026-02-20' })
    expect(wrapper.text()).toContain('共 1 条') // 边界含：仅 txn-2
  })

  it('选择类型即重新查询：kind 正确传后端', async () => {
    const wrapper = await mountView()
    await setKind(wrapper, 'income')
    expect(lastListFilter()).toMatchObject({ kind: 'income' })
    expect(wrapper.text()).toContain('共 1 条')
  })

  it('多条件组合：账户 + 日期 + 类型同时传入后端', async () => {
    const wrapper = await mountView()
    await setAccount(wrapper, 'acc-1')
    await setDateFrom(wrapper, '2026-01-01')
    await setDateTo(wrapper, '2026-03-31')
    await setKind(wrapper, 'transfer')
    expect(lastListFilter()).toMatchObject({
      involving_account_id: 'acc-1',
      from: '2026-01-01',
      to: '2026-03-31',
      kind: 'transfer',
    })
    expect(wrapper.text()).toContain('共 1 条') // 唯一同时命中：txn-3
  })

  it('清除筛选复位全部条件并回到全量列表（第 1 页）', async () => {
    const wrapper = await mountView()
    await setAccount(wrapper, 'acc-1')
    await setKind(wrapper, 'transfer')
    await setDateFrom(wrapper, '2026-01-01')
    expect(wrapper.text()).toContain('共 1 条')
    // 先翻页，验证清除后回到第 1 页
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2, involving_account_id: 'acc-1' })
    await clearButton(wrapper).trigger('click')
    await flushPromises()
    const f = lastListFilter()
    expect(f).toMatchObject({ page: 1, page_size: 20 })
    expect(f).not.toHaveProperty('from')
    expect(f).not.toHaveProperty('to')
    expect(f).not.toHaveProperty('involving_account_id')
    expect(f).not.toHaveProperty('kind')
    expect(wrapper.text()).toContain('共 5 条')
  })

  it('手动改动过滤不回写 URL（组件状态为唯一事实源）', async () => {
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    await setAccount(wrapper, 'acc-2')
    await setKind(wrapper, 'income')
    await setDateFrom(wrapper, '2026-01-01')
    expect(routeMock.query).toEqual({ account: 'acc-1' })
  })

  it('侧边栏重进（清除 account 参数）同时复位日期/类型过滤，回到全量列表（#96 决策 3）', async () => {
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    await setDateFrom(wrapper, '2026-02-01')
    await setKind(wrapper, 'income')
    expect(lastListFilter()).toMatchObject({
      involving_account_id: 'acc-1',
      from: '2026-02-01',
      kind: 'income',
    })
    // 模拟从侧边栏重新进入交易页：导航清除 query
    routeMock.query = {}
    await flushPromises()
    const f = lastListFilter()
    expect(f).toMatchObject({ page: 1, page_size: 20 })
    expect(f).not.toHaveProperty('involving_account_id')
    expect(f).not.toHaveProperty('from')
    expect(f).not.toHaveProperty('kind')
    expect(wrapper.text()).toContain('共 5 条')
  })

  it('参考数据重拉不把手动改动覆盖回 URL 值（URL 初始化仅结算一次）', async () => {
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    await setAccount(wrapper, 'acc-2')
    expect(lastListFilter()).toMatchObject({ involving_account_id: 'acc-2' })
    // 触发一次参考数据重拉（status loading → ready，如 ledger:changed 后的重载）
    await useReferenceStore().refresh()
    await flushPromises()
    // 手动改动保持，不被 URL 值 acc-1 覆盖
    expect(lastListFilter()).toMatchObject({ involving_account_id: 'acc-2' })
  })

  it('分页与页大小切换保持过滤条件', async () => {
    const wrapper = await mountView()
    await setAccount(wrapper, 'acc-1') // 涉及 acc-1：txn-1 / txn-3 / txn-5 共 3 条
    // 页大小切换：保持 acc-1 过滤并回到第 1 页
    tablePagination(wrapper).onUpdatePageSize(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({
      page: 1,
      page_size: 2,
      involving_account_id: 'acc-1',
    })
    expect(bodyRows(wrapper).length).toBe(2)
    // 翻页：保持过滤
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({
      page: 2,
      page_size: 2,
      involving_account_id: 'acc-1',
    })
    expect(wrapper.text()).toContain('共 3 条')
    expect(bodyRows(wrapper).length).toBe(1)
  })

  it('过滤无结果时展示空态提示（与加载态区分），空态可一键清除', async () => {
    const wrapper = await mountView()
    await setKind(wrapper, 'buy') // richDb 无 buy → 空结果
    expect(wrapper.text()).toContain('没有符合条件的交易')
    expect(bodyRows(wrapper).length).toBe(0)
    // 空态中的「清除筛选」可一键复位到全量
    await clearButton(wrapper).trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('共 5 条')
  })
})

describe('TransactionsView 转账行双向账户名（issue #99）', () => {
  // 混合数据集：转账行（txn-2: acc-2 → acc-1）与普通行并存，供双向展示 / 单账户名断言
  const mixedDb: Transaction[] = [
    makeTxn(1, 'acc-1', { kind: 'expense' }),
    makeTxn(2, 'acc-2', { kind: 'transfer', to_account_id: 'acc-1' }),
    makeTxn(3, 'acc-1', { kind: 'income' }),
  ]

  beforeEach(() => {
    txnDb = [...mixedDb]
  })

  /** 类型下拉（过滤行第 2 个 NSelect）直接 emit 变更（与 issue #98 测试同模式）。 */
  async function filterKind(wrapper: ReturnType<typeof mount>, k: string | null) {
    wrapper.findAllComponents(NSelect)[1].vm.$emit('update:value', k)
    await flushPromises()
  }

  it('转账行账户列显示「转出 → 转入」双向账户名，两个名字各自可点击、各自跳转对应账户', async () => {
    const wrapper = await mountView()
    await filterKind(wrapper, 'transfer')
    // 双向展示：两个账户名（转出 acc-2、转入 acc-1）+ 箭头分隔
    const links = wrapper.findAllComponents(AccountLink)
    expect(links.length).toBe(2)
    expect(links.map((l) => l.text())).toEqual(['银行', '现金'])
    expect(wrapper.text()).toContain('→')
    // 转出账户点击 → 跳转其过滤视图
    await links[0].find('button').trigger('click')
    expect(pushMock).toHaveBeenLastCalledWith({
      name: 'transactions',
      query: { account: 'acc-2' },
    })
    // 转入账户点击 → 跳转其过滤视图
    await links[1].find('button').trigger('click')
    expect(pushMock).toHaveBeenLastCalledWith({
      name: 'transactions',
      query: { account: 'acc-1' },
    })
  })

  it('非转账行账户列仍显示单个主账户名（可点击，带 title 提示）', async () => {
    const wrapper = await mountView()
    await filterKind(wrapper, 'income')
    const links = wrapper.findAllComponents(AccountLink)
    expect(links.length).toBe(1)
    expect(links[0].text()).toBe('现金')
    expect(links[0].attributes('title')).toBe('查看该账户的交易')
    await links[0].find('button').trigger('click')
    expect(pushMock).toHaveBeenLastCalledWith({
      name: 'transactions',
      query: { account: 'acc-1' },
    })
  })
})

describe('TransactionsView 记一笔 Modal（issue #141）', () => {
  /** 打开「记一笔」弹窗（点击工具栏按钮后等弹窗挂载）。 */
  async function openCreateModal(wrapper: ReturnType<typeof mount>) {
    const btn = wrapper.findAll('button').find((b) => b.text().includes('记一笔'))!
    await btn.trigger('click')
    await flushPromises()
  }

  it('点击「记一笔」打开 Modal，内嵌现有交易表单组件', async () => {
    const wrapper = await mountView()
    // 初始关闭：无 Modal、无表单
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    expect(wrapper.findComponent(TransactionForm).exists()).toBe(false)
    await openCreateModal(wrapper)
    expect(wrapper.findComponent(NModal).props('show')).toBe(true)
    // 弹窗内嵌的是现有 TransactionForm（含 6 种 kind 切换）
    const form = wrapper.findComponent(TransactionForm)
    expect(form.exists()).toBe(true)
  })

  it('提交成功后弹窗关闭、回到第 1 页并立即刷新（新记录可见）', async () => {
    const wrapper = await mountView()
    // 先翻到第 2 页再记一笔：成功后应回到第 1 页，确保新记录可见
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    await openCreateModal(wrapper)
    const before = listCalls().length
    // 表单提交成功 → created 事件
    wrapper.findComponent(TransactionForm).vm.$emit('created')
    await flushPromises()
    // 弹窗关闭（naive-ui Modal 关闭后内容保留在 DOM 仅隐藏，与 CategoryEditModal 同模式）
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    // 立即以第 1 页重新查询（筛选条件保留）
    expect(listCalls().length).toBe(before + 1)
    expect(lastListFilter()).toMatchObject({ page: 1 })
  })

  it('真实提交链路：弹窗内填表提交 → create_transaction → 弹窗关闭并刷新', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    // 默认 kind=expense → CategoryForm；弹窗表单在 TransactionForm 子树内定位
    const form = wrapper.findComponent(TransactionForm)
    // 金额（NInputNumber）与账户（CategoryForm 内第 2 个 NSelect，第 1 个是币种）
    form.getComponent(NInputNumber).vm.$emit('update:value', 12.5)
    form.findAllComponents(NSelect)[1].vm.$emit('update:value', 'acc-1')
    await flushPromises()
    mockInvoke.mockImplementationOnce((cmd: string) => {
      if (cmd === 'create_transaction') return Promise.resolve('new-id')
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    // 点击提交按钮「记支出」
    const submitBtn = form
      .findAllComponents(NButton)
      .find((b) => b.text().includes('记支出'))!
    await submitBtn.trigger('click')
    await flushPromises()
    // 后端收到正确账目
    const createCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')
    expect(createCalls).toHaveLength(1)
    const [, args] = createCalls[0] as [string, { input: Record<string, unknown> }]
    expect(args.input).toMatchObject({
      kind: 'expense',
      amount_cents: 1250,
      currency_code: 'CNY',
      account_id: 'acc-1',
    })
    // 弹窗关闭且列表刷新（回到第 1 页）
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    expect(lastListFilter()).toMatchObject({ page: 1 })
  })

  it('仅关闭弹窗（不提交）不触发列表刷新', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    const before = listCalls().length
    // 用户点遮罩/关闭 → update:show=false
    wrapper.findComponent(NModal).vm.$emit('update:show', false)
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    expect(listCalls().length).toBe(before)
  })
})
