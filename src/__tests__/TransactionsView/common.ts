import { vi, beforeEach, afterEach } from 'vitest'
import { mockInvoke } from '../helpers/invoke-mock'
import { fireProp } from '../helpers/component-vm'
import { DOMWrapper, mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { h, reactive } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { NDataTable, NDropdown, NDialogProvider } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import { stubReferenceInvoke } from '../helpers/reference-stubs'
import TransactionsView from '@/views/TransactionsView.vue'
import type { Account, Currency, Merchant, ReportDateRange, Transaction } from '@/types'

export { mockInvoke } from '../helpers/invoke-mock'

// 拆分后主题测试文件对导入绑定只读，可变模块态经 setter 改写。
export function setMerchantDb(rows: Merchant[]) {
  merchantDb = rows
}
export function setTxnDb(rows: Transaction[]) {
  txnDb = rows
}
/** 账户库可变：借贷呈现测试注入 receivable/debt 账户（issue #374）。 */
export function setAccountDb(rows: Account[]) {
  mockAccounts = rows
}

/** report_date_range 覆盖（issue #391 视图测试）：注入自定义 Promise 模拟边界
 * 拉取失败（reject）/在途（永不 resolve）；null = 恢复默认（按 txnDb 推导极值）。 */
let reportDateRangeOverride: Promise<ReportDateRange> | null = null
export function setReportDateRange(value: Promise<ReportDateRange> | null) {
  reportDateRangeOverride = value
}

/** 商户字典（可变：软删商户显示测试会清空它模拟 list_merchants 的新返回）。 */
export let merchantDb: Merchant[] = [
  {
    id: 'mch-1', name: '京东',
    updated_at: '2026-01-01T00:00:00Z',
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

/** 账户库（可变，issue #374 借贷呈现测试经 setAccountDb 注入 receivable/debt 账户）。 */
export let mockAccounts: Account[] = [
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
    merchant_id: null,
    policy_id: null,
    refund_of_transaction_id: null,
    note: `备注 ${i}`,
    date: '2026-01-01',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    source: null,
    ...overrides,
  }
}

/** 可变的交易库（模块内部态，主题测试经 setTxnDb 改写）：删除操作会真实移除，
 * 分页返回随 total 变化。偶数序号在 acc-2、奇数序号在 acc-1，供涉及账户过滤断言。 */
let txnDb: Transaction[] = []

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
    // 镜像后端读接缝（issue #377/#581）：精确分类 / 仅无分类 / 类型集合
    if (filter.category_id && t.category_id !== (filter.category_id as string)) return false
    if (filter.uncategorized_only === true && t.category_id !== null) return false
    if (Array.isArray(filter.kinds) && !filter.kinds.includes(t.kind)) return false
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
  stubReferenceInvoke({
    // 参考数据：只覆写本视图实际行使的命令；币种走规范夹具（同 mockCurrencies）。
    // 可变库（setAccountDb/setMerchantDb 改写）用函数型覆写，派发时取最新值。
    list_accounts: () => mockAccounts,
    list_categories: [],
    list_merchants: () => merchantDb,
    list_policies: [],
    list_items: [],
    report_date_range: () => {
      // 数据期间边界（issue #391）：默认与后端口径一致（MIN/MAX 日期，随 txnDb 现算）
      if (reportDateRangeOverride) return reportDateRangeOverride
      const dates = txnDb.map((t) => t.date).sort()
      return Promise.resolve({ min_date: dates[0] ?? null, max_date: dates[dates.length - 1] ?? null })
    },
    list_transactions: (args?: { filter?: Record<string, unknown> }) => {
      const filter = args?.filter ?? {}
      const scoped = applyListFilter(filter)
      const pageSize = (filter.page_size as number) ?? scoped.length
      const page = (filter.page as number) ?? 1
      const start = (page - 1) * pageSize
      return Promise.resolve({
        items: scoped.slice(start, start + pageSize),
        total: scoped.length,
      })
    },
    delete_transaction: (args?: { id?: string }) => {
      txnDb = txnDb.filter((t) => t.id !== args?.id)
      return Promise.resolve()
    },
  })
  merchantDb = [
    {
      id: 'mch-1', name: '京东',
      updated_at: '2026-01-01T00:00:00Z',
      version: 1, device_id: 'test', is_deleted: false,
    },
  ]
  reportDateRangeOverride = null
  localStorage.clear()
  const store = useReferenceStore()
  await store.refresh()
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

/** 菜单选择（走 NDropdown 的 onSelect 装配缝，经 fireProp 单点窄化）。 */
export async function selectRowMenu(wrapper: ReturnType<typeof mount>, key: string) {
  fireProp(rowMenu(wrapper), 'onSelect', key)
  await flushPromises()
}

/** 过滤 v-show 隐藏容器（jsdom 中 leave 过渡不会结束会残留旧内容），只取可见节点。 */
function hasHiddenAncestor(el: Element): boolean {
  let node: Element | null = el
  while (node && node !== document.body) {
    if ((node as HTMLElement).style.display === 'none') return true
    node = node.parentElement
  }
  return false
}

function visibleNodes(selector: string): Element[] {
  return [...document.querySelectorAll(selector)].filter((el) => !hasHiddenAncestor(el))
}

/** 确认/取消删除对话框。useDialog 的对话框经 NModal teleport 到 body，
 * 需从 document 查询；同一 modal 容器在 jsdom 中会残留，只取可见节点。 */
function visibleDialogButtons() {
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

/** 确认框遮罩「按下-抬起」完整事件序列：真实浏览器中按下-抬起在遮罩上合成 click
 * 触发关闭判定，jsdom 不自动合成，手动派发三段事件等价模拟
 * （AppModal 契约测试同款先例）。 */
export async function pressReleaseOnDialogMask() {
  const mask = document.body.querySelector('.n-modal-mask')
  expect(mask, '.n-modal-mask 应存在').not.toBeNull()
  mask!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
  mask!.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))
  mask!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  await flushPromises()
}
