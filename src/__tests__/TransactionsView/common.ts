import { vi, beforeEach, afterEach } from 'vitest'
import { DOMWrapper, mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { h, reactive } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setActivePinia, createPinia } from 'pinia'
import { NDataTable, NDropdown, NDialogProvider } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import TransactionsView from '@/views/TransactionsView.vue'
import type { Account, Currency, Merchant, Transaction } from '@/types'

export const mockInvoke = vi.mocked(invoke)

// 拆分后主题测试文件对导入绑定只读，可变模块态经 setter 改写。
export function setMerchantDb(rows: Merchant[]) {
  merchantDb = rows
}
export function setTxnDb(rows: Transaction[]) {
  txnDb = rows
}

/** 商户字典（可变：软删商户显示测试会清空它模拟 list_merchants 的新返回）。 */
export let merchantDb: Merchant[] = [
  {
    id: 'mch-1', name: '京东', icon: null, color: null,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
]

// 测试挂载的视图不手动 unmount：useCreateShortcuts/useWindowGuard 会在 window/document
// 上注册监听、仅在 unmount 时移除。不卸载会让监听跨测试累积，裸键快捷键用例互相污染。
enableAutoUnmount(afterEach)

// 路由 mock：TransactionsView 经 useRoute 读取 URL query（?account=<id> 只读入口）。
// 测试通过改写 routeMock.query 模拟带参/不带参进入与 query 变化；
// AccountLink 经 useRouter 跳转（pushMock 断言导航目标，issue #97/#99）。
export const routeMock = reactive<{ query: Record<string, string | string[] | null> }>({ query: {} })
export const pushMock = vi.fn()
vi.mock('vue-router', () => ({
  useRoute: () => routeMock,
  useRouter: () => ({ push: pushMock }),
}))

export const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

export const mockAccounts: Account[] = [
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

export function makeTxn(i: number, accountId = 'acc-1', overrides: Partial<Transaction> = {}): Transaction {
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
export let txnDb: Transaction[] = []

/** 与后端 read.rs 口径一致：涉及账户 / 商户 / 日期起止 / 类型 / 分页 AND 组合过滤。 */
export function applyListFilter(filter: Record<string, unknown>) {
  return txnDb.filter((t) => {
    if (filter.involving_account_id) {
      const id = filter.involving_account_id as string
      if (t.account_id !== id && t.to_account_id !== id) return false
    }
    if (filter.merchant_id && t.merchant_id !== filter.merchant_id) return false
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
    if (cmd === 'list_merchants') return Promise.resolve(merchantDb)
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
    // 物品 store（issue #119 右键「加入物品」置灰态）默认空列表
    if (cmd === 'list_items') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  merchantDb = [
    {
      id: 'mch-1', name: '京东', icon: null, color: null,
      created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
      version: 1, device_id: 'test', is_deleted: false,
    },
  ]
  localStorage.clear()
  const store = useReferenceStore()
  await store.ensureFresh()
})

export async function mountView() {
  const wrapper = mountViewSync()
  await flushPromises()
  return wrapper
}

/** 视图顶层调用 useDialog（issue #151 删除二次确认），与 App.vue 同构需 NDialogProvider 包裹。 */
export function mountViewSync() {
  return mount(NDialogProvider, {
    slots: { default: () => h(TransactionsView) },
  })
}

export function listCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_transactions')
}

export function lastListFilter() {
  const calls = listCalls()
  const [, args] = calls[calls.length - 1] as [string, { filter: Record<string, unknown> }]
  return args.filter
}

export function tablePagination(wrapper: ReturnType<typeof mount>) {
  return wrapper.findComponent(NDataTable).props('pagination') as {
    page: number
    pageSize: number
    itemCount: number
    onChange: (page: number) => void
    onUpdatePageSize: (pageSize: number) => void
  }
}

export function bodyRows(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAll('.n-data-table-tbody .n-data-table-tr')
}

export function deleteCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'delete_transaction')
}

export function createCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')
}

/** 右键指定行打开上下文菜单（issue #151）。 */
export async function openMenuOnRow(wrapper: ReturnType<typeof mount>, index = 0) {
  await bodyRows(wrapper)[index].trigger('contextmenu')
  await flushPromises()
}

export function rowMenu(wrapper: ReturnType<typeof mount>) {
  // 视图上有多个 NDropdown（#150 记一笔分裂按钮 + #151 行右键菜单），
  // 按菜单项含 delete key 识别行右键菜单
  return wrapper.findAllComponents(NDropdown).find((d) =>
    (d.props('options') as Array<{ key?: string }>).some((o) => o.key === 'delete'),
  )!
}

export function rowMenuKeys(wrapper: ReturnType<typeof mount>) {
  return (rowMenu(wrapper).props('options') as Array<{ key: string }>).map((o) => o.key)
}

/** 菜单选择（走 NDropdown 的 onSelect 装配缝）。 */
export async function selectRowMenu(wrapper: ReturnType<typeof mount>, key: string) {
  rowMenu(wrapper).props('onSelect')?.(key)
  await flushPromises()
}

/** 过滤 v-show 隐藏容器（jsdom 中 leave 过渡不会结束会残留旧内容），只取可见节点。 */
export function hasHiddenAncestor(el: Element): boolean {
  let node: Element | null = el
  while (node && node !== document.body) {
    if ((node as HTMLElement).style.display === 'none') return true
    node = node.parentElement
  }
  return false
}

export function visibleNodes(selector: string): Element[] {
  return [...document.querySelectorAll(selector)].filter((el) => !hasHiddenAncestor(el))
}

/** 确认/取消删除对话框。useDialog 的对话框经 NModal teleport 到 body，
 * 需从 document 查询；同一 modal 容器在 jsdom 中会残留，只取可见节点。 */
export function visibleDialogButtons() {
  return visibleNodes('.n-dialog button')
}

export function dialogText(): string {
  return visibleNodes('.n-dialog')
    .map((el) => el.textContent ?? '')
    .join('')
}

/** 弹窗内容文本：NModal teleport 到 body 且组件根为占位符，需从 document 查卡片。 */
export function visibleModalText(): string {
  return visibleNodes('.n-card')
    .map((el) => el.textContent ?? '')
    .join('')
}

export async function clickDialogButton(text: string) {
  const btn = visibleDialogButtons().find((el) => el.textContent?.trim() === text)!
  await new DOMWrapper(btn).trigger('click')
  await flushPromises()
}
