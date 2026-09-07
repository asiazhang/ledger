import { vi, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from '../helpers/invoke-mock'
import { fireProp } from '../helpers/component-vm'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { reactive } from 'vue'
import { NDataTable, NDropdown } from 'naive-ui'
import { mountWithDialog } from '../helpers/mount'
import TransactionsView from '@/views/TransactionsView.vue'
import type { Account, Merchant, ReportDateRange, Transaction } from '@/types'

/**
 * TransactionsView 测试目录薄壳（issue #748，ADR-0085 决策 7）：只承载本目录
 * 特有夹具与编排组合——可变 db、路由替身、后端读取口径镜像、表格/行菜单编排。
 * 通用布线（invoke 接缝 wireInvokeSeam）、清理四件套（全局壳层每测自动执行）、
 * 挂载与 DOM 查找（helpers/mount、helpers/dom）一律上收测试辅助层，禁止薄壳
 * 再生长通用能力。
 */

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

// 路由 mock：TransactionsView 经 useRoute 读取 URL query（?account=<id> 只读入口）。
// 测试通过改写 routeMock.query 模拟带参/不带参进入与 query 变化；
// AccountLink 经 useRouter 跳转（pushMock 断言导航目标，issue #97/#99）。
export const routeMock = reactive<{ query: Record<string, string | string[] | null> }>({ query: {} })
export const pushMock = vi.fn()
vi.mock('vue-router', () => ({
  useRoute: () => routeMock,
  useRouter: () => ({ push: pushMock }),
}))

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
function applyListFilter(filter: Record<string, unknown>) {
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
  pushMock.mockReset()
  routeMock.query = {}
  txnDb = Array.from({ length: 45 }, (_, i) =>
    makeTxn(i + 1, i % 2 === 0 ? 'acc-2' : 'acc-1'),
  )
  merchantDb = [
    {
      id: 'mch-1', name: '京东',
      updated_at: '2026-01-01T00:00:00Z',
      version: 1, device_id: 'test', is_deleted: false,
    },
  ]
  reportDateRangeOverride = null
  // 唯一接缝布线（ADR-0085）：defaults 表只收本目录场景的静态契约；可变库与
  // 行为编排（镜像过滤、删除移除、边界在途/失败）为函数型 overrides。参考字典
  // 五命令不在此枚举——桩层规范夹具兜底（币种走规范夹具，账户/商户以目录夹具
  // 覆写）。store 层预热 opt-in 开启：视图用例依赖参考数据就绪后的即时渲染。
  const seam = wireInvokeSeam({
    defaults: {
      list_categories: [],
      list_policies: [],
      list_items: [],
    },
    overrides: {
      // 可变库（setAccountDb/setMerchantDb 改写）用函数型覆写，派发时取最新值。
      list_accounts: () => mockAccounts,
      list_merchants: () => merchantDb,
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
    },
    refreshReferenceStores: true,
  })
  await seam.ready
})

export async function mountView() {
  const wrapper = mountViewSync()
  await flushPromises()
  return wrapper
}

/** 视图顶层调用 useDialog（issue #151 删除二次确认），与 App.vue 同构需 NDialogProvider 包裹。 */
export function mountViewSync() {
  return mountWithDialog(TransactionsView)
}

export function listCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_transactions')
}

export function lastListFilter() {
  const calls = listCalls()
  const [, args] = calls[calls.length - 1] as [string, { filter: Record<string, unknown> }]
  return args.filter
}

export function tablePagination(wrapper: VueWrapper) {
  return wrapper.findComponent(NDataTable).props('pagination') as {
    page: number
    pageSize: number
    itemCount: number
    onChange: (page: number) => void
    onUpdatePageSize: (pageSize: number) => void
  }
}

export function bodyRows(wrapper: VueWrapper) {
  return wrapper.findAll('.n-data-table-tbody .n-data-table-tr')
}

export function deleteCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'delete_transaction')
}

export function createCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')
}

/** 右键指定行打开上下文菜单（issue #151）。 */
export async function openMenuOnRow(wrapper: VueWrapper, index = 0) {
  await bodyRows(wrapper)[index].trigger('contextmenu')
  await flushPromises()
}

export function rowMenu(wrapper: VueWrapper) {
  // 视图上有多个 NDropdown（#150 记一笔分裂按钮 + #151 行右键菜单），
  // 按菜单项含 delete key 识别行右键菜单
  return wrapper.findAllComponents(NDropdown).find((d) =>
    (d.props('options') as Array<{ key?: string }>).some((o) => o.key === 'delete'),
  )!
}

export function rowMenuKeys(wrapper: VueWrapper) {
  return (rowMenu(wrapper).props('options') as Array<{ key: string }>).map((o) => o.key)
}

/** 菜单选择（走 NDropdown 的 onSelect 装配缝，经 fireProp 单点窄化）。 */
export async function selectRowMenu(wrapper: VueWrapper, key: string) {
  fireProp(rowMenu(wrapper), 'onSelect', key)
  await flushPromises()
}
