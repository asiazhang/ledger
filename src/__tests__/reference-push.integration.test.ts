import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { NDialogProvider, NSelect, NTreeSelect } from 'naive-ui'
import { h, reactive } from 'vue'
import { useReferenceStore } from '@/stores/reference'
import CategoryManager from '@/components/CategoryManager.vue'
import CategoryForm from '@/components/CategoryForm.vue'
import TransactionsView from '@/views/TransactionsView.vue'
import type { Account, Category, Currency, Transaction } from '@/types'

// TransactionsView 经 useRoute 读取 URL query（?account=<id> 只读入口，issue #97）；
// 本文件挂载该视图但无路由上下文，mock 为空 query（无账户过滤，不影响既有断言）。
// AccountLink（账户列渲染）经 useRouter 跳转（issue #99），同样 mock 为 no-op。
const routeMock = reactive<{ query: Record<string, string | string[] | null> }>({ query: {} })
vi.mock('vue-router', () => ({
  useRoute: () => routeMock,
  useRouter: () => ({ push: () => {} }),
}))

// issue #86：端到端整合验证 + 组件层反应性测试。
// 场景骨架：外部 AI 经本地 HTTP API 写入参考数据（账户/分类）→ 后端成功 emit `ledger:changed`
// （issue #79 的薄胶，见 src-tauri/src/events.rs / api_server.rs）→ useReferenceStore 收到事件
// 重拉三表（stale-while-revalidate）→ 已挂载的视图/表单经响应式状态自动呈现新数据。
// 测试主缝与 spec #76 一致：`invoke`（数据访问）与 `listen`（事件订阅），无需真实 Tauri/HTTP。

const mockInvoke = vi.mocked(invoke)
const mockListen = vi.mocked(listen)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1', name: '现金', type: 'cash', currency_code: 'CNY',
    initial_balance_cents: 0, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false, is_hidden: false,
  },
  {
    id: 'acc-2', name: '招商银行', type: 'bank', currency_code: 'CNY',
    initial_balance_cents: 100000, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false, is_hidden: false,
  },
]

const mockCategories: Category[] = [
  {
    id: 'cat-food', name: '餐饮', kind: 'expense', parent_id: null,
    icon: null, sort_order: 0, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
  {
    id: 'cat-salary', name: '工资', kind: 'income', parent_id: null,
    icon: null, sort_order: 0, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
]

function makeTxn(i: number, categoryId: string | null): Transaction {
  return {
    id: `txn-${String(i).padStart(3, '0')}`,
    kind: 'expense',
    amount_cents: i * 100,
    currency_code: 'CNY',
    amount_native_cents: i * 100,
    account_id: 'acc-1',
    to_account_id: null,
    category_id: categoryId,
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

/** 外部 AI 导入后新增的参考数据 fixture（与基准数据同型，模拟写后数据库状态）。 */
const importedAccount: Account = {
  id: 'acc-ai', name: 'AI 导入账户', type: 'bank', currency_code: 'USD',
  initial_balance_cents: 0, created_at: '2026-02-01T00:00:00Z',
  updated_at: '2026-02-01T00:00:00Z', version: 1, device_id: 'test',
  is_deleted: false, is_hidden: false,
}
const importedCategory: Category = {
  id: 'cat-ai', name: 'AI 导入分类', kind: 'expense', parent_id: null,
  icon: null, sort_order: 1, created_at: '2026-02-01T00:00:00Z',
  updated_at: '2026-02-01T00:00:00Z', version: 1, device_id: 'test',
  is_deleted: false,
}

let changedHandlers: Array<(...args: unknown[]) => void> = []

/** 参考数据 list_* 命令的默认 mock 实现；overrides 可替换为写后数据库状态。 */
function listImpl(overrides: {
  accounts?: Account[]
  categories?: Category[]
} = {}): (cmd: string) => Promise<unknown> {
  return (cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(overrides.accounts ?? mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve(overrides.categories ?? mockCategories)
    if (cmd === 'list_merchants') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  }
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  mockInvoke.mockImplementation(listImpl())
  mockListen.mockImplementation((_event: string, handler: (...args: unknown[]) => void) => {
    // 多个 store（reference / items 等）各自订阅同一信号，全部捕获、全部触发
    changedHandlers.push(handler)
    return Promise.resolve(vi.fn())
  })
  localStorage.clear()
  // 先访问 store 以捕获 listen 回调（组件复用同一 store 单例），再确保数据就绪
  const store = useReferenceStore()
  await store.ensureFresh()
})

afterEach(() => {
  changedHandlers = []
})

/**
 * 模拟「外部 AI 经 HTTP API 写入参考数据」完成后，后端 emit `ledger:changed` 的完整链路：
 * 1. 改写 invoke mock，使下一次重拉返回「写后数据库状态」；
 * 2. 触发捕获到的 `ledger:changed` 回调（无 payload，与后端发射一致）；
 * 3. 等待 store 重拉完成（三表整体替换）。
 */
async function simulateExternalWrite(patch: {
  accounts?: Account[]
  categories?: Category[]
}) {
  mockInvoke.mockImplementation(listImpl(patch))
  changedHandlers.forEach((h) => h({ payload: undefined }))
  await flushPromises()
}

describe('组件层反应性：mock ledger:changed 使界面/选项原地更新（issue #86 AC2）', () => {
  it('分类管理树：外部 AI 导入新分类后，已打开的分类树原地出现新分类', async () => {
    const wrapper = mount(CategoryManager)
    expect(wrapper.text()).toContain('餐饮')
    expect(wrapper.text()).not.toContain('AI 导入分类')

    // 同一 wrapper 不重挂载：仅触发 push
    await simulateExternalWrite({ categories: [...mockCategories, importedCategory] })

    expect(wrapper.text()).toContain('AI 导入分类')
    expect(wrapper.text()).toContain('餐饮') // 旧数据仍在（增量而非清空）
  })

  it('表单选项：外部 AI 导入新账户后，已打开的交易表单账户下拉原地出现新选项', async () => {
    const wrapper = mount(CategoryForm, { props: { kind: 'expense', submitLabel: '记支出' } })
    const accountSelect = wrapper.findAllComponents(NSelect)[1] // 账户下拉（第 1 个为币种）
    expect(accountSelect.props('options').map((o: { value: string }) => o.value))
      .toEqual(['acc-1', 'acc-2'])

    await simulateExternalWrite({ accounts: [...mockAccounts, importedAccount] })

    const options = accountSelect.props('options') as { value: string; label: string }[]
    expect(options.map((o) => o.value)).toEqual(['acc-1', 'acc-2', 'acc-ai'])
    expect(options.map((o) => o.label)).toContain('AI 导入账户')
  })

  it('表单选项：外部 AI 导入新分类后，分类树选择器原地出现新分类', async () => {
    const wrapper = mount(CategoryForm, { props: { kind: 'expense', submitLabel: '记支出' } })
    const treeSelect = wrapper.findAllComponents(NTreeSelect)[0]
    expect(treeSelect.props('options').map((o: { key: string }) => o.key)).toEqual(['cat-food'])

    await simulateExternalWrite({ categories: [...mockCategories, importedCategory] })

    expect(treeSelect.props('options').map((o: { key: string }) => o.key))
      .toEqual(['cat-food', 'cat-ai'])
  })

  it('交易列表映射渲染：外部 AI 更新分类名后，已打开的交易列表分类列原地显示新名称', async () => {
    const txnDb: Transaction[] = [makeTxn(1, 'cat-food'), makeTxn(2, 'cat-food')]
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_transactions') {
        return Promise.resolve({ items: txnDb, total: txnDb.length })
      }
      return listImpl()(cmd)
    })

    // TransactionsView 顶层调用 useDialog（issue #151），需 NDialogProvider 包裹（同 App.vue）
    const wrapper = mount(NDialogProvider, {
      slots: { default: () => h(TransactionsView) },
    })
    await flushPromises()
    expect(wrapper.text()).toContain('餐饮')

    // 外部 AI 将分类「餐饮」改名为「夜宵」（update_category 成功 emit ledger:changed）
    const renamed = mockCategories.map((c) =>
      c.id === 'cat-food' ? { ...c, name: '夜宵' } : c,
    )
    await simulateExternalWrite({ categories: renamed })

    // 分类列渲染闭包读取 categoryPath（响应式），同一 wrapper 原地显示新名称（无用户操作、无重挂载）
    expect(wrapper.text()).toContain('夜宵')
    expect(wrapper.text()).not.toContain('餐饮')
  })

  it('stale-while-revalidate：重拉期间界面保留旧数据，完成后整体替换', async () => {
    const wrapper = mount(CategoryManager)
    expect(wrapper.text()).toContain('餐饮')

    let resolveCats!: (v: Category[]) => void
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_categories') {
        return new Promise((res) => {
          resolveCats = res
        })
      }
      return listImpl()(cmd)
    })

    // 触发 push：重拉挂起期间，已打开的分类树不闪空（旧数据原样保留）
    changedHandlers.forEach((h) => h({ payload: undefined }))
    await flushPromises()
    expect(wrapper.text()).toContain('餐饮')
    expect(wrapper.text()).not.toContain('AI 导入分类')

    resolveCats([...mockCategories, importedCategory])
    await flushPromises()

    expect(wrapper.text()).toContain('AI 导入分类')
    expect(wrapper.text()).toContain('餐饮')
  })
})

describe('端到端整合验证：外部 AI 导入账户/分类 → 已打开界面自动呈现新数据（issue #86 AC1）', () => {
  it('批量导入新账户+新分类：已打开的分类树与账户下拉同步原地更新，失效信号可观测', async () => {
    // 用户已打开两个界面：分类管理树 + 交易录入表单（账户下拉）
    const treeWrapper = mount(CategoryManager)
    const formWrapper = mount(CategoryForm, { props: { kind: 'expense', submitLabel: '记支出' } })
    expect(treeWrapper.text()).toContain('餐饮')
    expect(treeWrapper.text()).not.toContain('AI 导入')
    const accountSelect = formWrapper.findAllComponents(NSelect)[1]
    expect(accountSelect.props('options').map((o: { value: string }) => o.value))
      .toEqual(['acc-1', 'acc-2'])
    const store = useReferenceStore()
    expect(store.version).toBe(1)

    // 外部 AI 一次性导入账户 + 分类（POST /api/v1/accounts、POST /api/v1/categories）
    // 后端在写成功后 emit `ledger:changed` → store 重拉 → 界面原地更新
    await simulateExternalWrite({
      accounts: [...mockAccounts, importedAccount],
      categories: [...mockCategories, importedCategory],
    })

    // 两个已打开界面无需重挂载即呈现新数据
    expect(treeWrapper.text()).toContain('AI 导入分类')
    const options = accountSelect.props('options') as { value: string; label: string }[]
    expect(options.map((o) => o.value)).toContain('acc-ai')
    expect(options.map((o) => o.label)).toContain('AI 导入账户')
    // 失效信号可观测：成功重拉 version 自增、status 回到 ready
    expect(store.version).toBe(2)
    expect(store.status).toBe('ready')
    // 派生映射随参考数据更新（表单选项的数据来源）
    expect(store.accountMap.get('acc-ai')?.name).toBe('AI 导入账户')
    expect(store.categoryMap.get('cat-ai')?.name).toBe('AI 导入分类')
  })
})
