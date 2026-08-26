import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { reactive } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { NDataTable, NPopconfirm } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import TransactionsView from '@/views/TransactionsView.vue'
import type { Account, Currency, Transaction } from '@/types'

const mockInvoke = vi.mocked(invoke)

// 路由 mock：TransactionsView 经 useRoute 读取 URL query（?account=<id> 只读入口）。
// 测试通过改写 routeMock.query 模拟带参/不带参进入与 query 变化。
const routeMock = reactive<{ query: Record<string, string | string[] | null> }>({ query: {} })
vi.mock('vue-router', () => ({
  useRoute: () => routeMock,
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

function makeTxn(i: number, accountId = 'acc-1'): Transaction {
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
  }
}

/** 可变的交易库：删除操作会真实移除，分页返回随 total 变化。
 * 偶数序号在 acc-2、奇数序号在 acc-1，供涉及账户过滤断言。 */
let txnDb: Transaction[] = []

function applyInvolvingFilter(accountId: string) {
  return txnDb.filter(
    (t) => t.account_id === accountId || t.to_account_id === accountId,
  )
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  routeMock.query = {}
  txnDb = Array.from({ length: 45 }, (_, i) =>
    makeTxn(i + 1, i % 2 === 0 ? 'acc-2' : 'acc-1'),
  )
  mockInvoke.mockImplementation((cmd: string, args?: { filter?: Record<string, unknown>; id?: string }) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_transactions') {
      const filter = args?.filter ?? {}
      const involving = filter.involving_account_id as string | undefined
      const scoped = involving ? applyInvolvingFilter(involving) : txnDb
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
