import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { nextTick } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { NDatePicker } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import SearchView from '@/views/SearchView.vue'
import AccountLink from '@/components/AccountLink.vue'
import type { Account, Category, Currency, Merchant, Transaction } from '@/types'

const mockInvoke = vi.mocked(invoke)


// AccountLink 经 useRouter 跳转（pushMock 断言导航目标，issue #99）
const pushMock = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push: pushMock }),
}))

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-cash',
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
    id: 'acc-bank',
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

const mockCategories: Category[] = [
  {
    id: 'cat-food',
    name: '餐饮',
    kind: 'expense',
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

const mockMerchants: Merchant[] = [
  {
    id: 'mer-jd',
    name: '京东',
    icon: null,
    color: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

function makeTransaction(
  id: string,
  note: string,
  date: string,
  amountCents: number,
  overrides: Partial<Transaction> = {},
): Transaction {
  return {
    id,
    kind: 'expense',
    amount_cents: amountCents,
    currency_code: 'CNY',
    amount_native_cents: amountCents,
    account_id: 'acc-cash',
    to_account_id: null,
    category_id: 'cat-food',
    refund_of_transaction_id: null,
    note,
    date,
    created_at: `${date}T00:00:00Z`,
    updated_at: `${date}T00:00:00Z`,
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...overrides,
  }
}

// 25 条：前 23 条备注「午餐」、后 2 条备注「报销」（跨 2 页，pageSize=20）。
// 金额随索引递增（1000 + i*100 分），日期逐日递增（2026-02-01 ~ 2026-02-25），供金额/日期筛选测试。
const mockTransactions: Transaction[] = [
  ...Array.from({ length: 25 }, (_, i) =>
    makeTransaction(
      `tx-${i + 1}`,
      i < 23 ? '午餐' : '报销',
      `2026-02-${String(i + 1).padStart(2, '0')}`,
      1000 + i * 100,
    ),
  ),
  // 转账交易（issue #99 双向账户名断言）：acc-cash → acc-bank（金额低于既有筛选测试阈值，避免影响命中数）
  makeTransaction('tx-tr', '转账', '2026-02-26', 1500, {
    kind: 'transfer',
    to_account_id: 'acc-bank',
  }),
  // 带商户交易（issue #193 搜索结果展示商户）：备注唯一、日期/金额避开既有筛选测试口径
  makeTransaction('tx-mer', '家电采购', '2026-03-01', 100, { merchant_id: 'mer-jd' }),
]

function searchCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'search_transactions')
}

function lastSearchArgs() {
  const calls = searchCalls()
  expect(calls.length).toBeGreaterThan(0)
  return calls[calls.length - 1][1] as {
    query: string
    page: number
    pageSize: number
    amountMinCents: number | null
    amountMaxCents: number | null
    dateFrom: string | null
    dateTo: string | null
  }
}

/** 输入关键字并等待防抖与异步搜索完成（fake timers 下微任务由 advance 驱动）。 */
async function typeAndSearch(wrapper: VueWrapper, text: string, delay = 300) {
  await wrapper.find('input').setValue(text)
  await nextTick()
  await vi.advanceTimersByTimeAsync(delay)
  await nextTick()
  await nextTick()
}

function minAmountInput(wrapper: VueWrapper) {
  const el = wrapper
    .findAll('input')
    .find((i) => i.attributes('placeholder')?.includes('最低金额'))
  expect(el).toBeTruthy()
  return el!
}

function maxAmountInput(wrapper: VueWrapper) {
  const el = wrapper
    .findAll('input')
    .find((i) => i.attributes('placeholder')?.includes('最高金额'))
  expect(el).toBeTruthy()
  return el!
}

/** 直接向 NDatePicker emit update:formattedValue 设置日期（避免在 fake timers 下打开面板）。 */
async function setDate(wrapper: VueWrapper, index: 0 | 1, value: string) {
  const pickers = wrapper.findAllComponents(NDatePicker)
  expect(pickers.length).toBe(2)
  pickers[index].vm.$emit('update:formattedValue', value)
  await nextTick()
}

async function applyFilters(wrapper: VueWrapper, delay = 300) {
  await vi.advanceTimersByTimeAsync(delay)
  await nextTick()
  await nextTick()
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  pushMock.mockReset()
  mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve(mockCategories)
    if (cmd === 'list_merchants') return Promise.resolve(mockMerchants)
    if (cmd === 'search_transactions') {
      const {
        query,
        page = 1,
        pageSize = 20,
        amountMinCents = null,
        amountMaxCents = null,
        dateFrom = null,
        dateTo = null,
      } = (args ?? {}) as {
        query?: string
        page?: number
        pageSize?: number
        amountMinCents?: number | null
        amountMaxCents?: number | null
        dateFrom?: string | null
        dateTo?: string | null
      }
      // 与后端一致：仅筛选（无关键字）也正常执行
      const all = mockTransactions.filter((t) => {
        if (query && !(t.note ?? '').includes(query)) return false
        if (amountMinCents != null && t.amount_cents < amountMinCents) return false
        if (amountMaxCents != null && t.amount_cents > amountMaxCents) return false
        // 日期为 YYYY-MM-DD 字符串，字典序即时间序（含边界）
        if (dateFrom && t.date < dateFrom) return false
        if (dateTo && t.date > dateTo) return false
        return true
      })
      const start = (page - 1) * pageSize
      return Promise.resolve({
        items: all.slice(start, start + pageSize),
        total: all.length,
      })
    }
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  localStorage.clear()
  const store = useReferenceStore()
  await store.ensureFresh()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('SearchView.vue', () => {
  it('空输入显示占位提示且不触发搜索', async () => {
    const wrapper = mount(SearchView)
    await flushPromises()
    expect(wrapper.text()).toContain('输入关键字或设置筛选开始搜索')
    expect(searchCalls().length).toBe(0)
  })

  it('筛选器 UI：最低/最高金额输入与起止日期选择器位于关键字下方', async () => {
    const wrapper = mount(SearchView)
    await flushPromises()
    const keywordInput = wrapper.find('input')
    expect(keywordInput.attributes('placeholder')).toContain('输入关键字')
    expect(minAmountInput(wrapper).attributes('placeholder')).toBe('最低金额（元）')
    expect(maxAmountInput(wrapper).attributes('placeholder')).toBe('最高金额（元）')
    expect(wrapper.findAllComponents(NDatePicker).length).toBe(2)
  })

  it('输入后防抖 300ms 才触发一次搜索', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '午餐', 299)
    expect(searchCalls().length).toBe(0)
    await vi.advanceTimersByTimeAsync(1)
    await nextTick()
    await nextTick()
    expect(searchCalls().length).toBe(1)
  })

  it('连续输入只触发一次搜索（防抖合并）', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await wrapper.find('input').setValue('午')
    await nextTick()
    await vi.advanceTimersByTimeAsync(100)
    await wrapper.find('input').setValue('午餐')
    await nextTick()
    await vi.advanceTimersByTimeAsync(300)
    await nextTick()
    await nextTick()
    expect(searchCalls().length).toBe(1)
    expect(lastSearchArgs().query).toBe('午餐')
  })

  it('搜索结果渲染表格并显示「命中 N 条」', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '报销')
    expect(wrapper.text()).toContain('命中 2 条')
    expect(wrapper.text()).toContain('报销')
    expect(wrapper.text()).toContain('¥33')
    expect(wrapper.text()).toContain('支出')
    expect(wrapper.text()).toContain('餐饮')
    expect(wrapper.text()).toContain('现金')
  })

  it('搜索结果展示商户（复用交易列表信息口径的商户列）', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '家电采购')
    expect(wrapper.text()).toContain('命中 1 条')
    // 商户列头与商户名（merchantMap 解析）均渲染
    expect(wrapper.text()).toContain('商户')
    expect(wrapper.text()).toContain('京东')
  })

  it('invoke(search_transactions) 参数正确（query/page/pageSize）', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '午餐')
    expect(lastSearchArgs()).toMatchObject({
      query: '午餐',
      page: 1,
      pageSize: 20,
      amountMinCents: null,
      amountMaxCents: null,
      dateFrom: null,
      dateTo: null,
    })
  })

  it('分页：点击第 2 页携带 page=2 重新搜索', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '午餐')
    expect(wrapper.text()).toContain('命中 23 条')

    const pageTwo = wrapper
      .findAll('.n-pagination-item')
      .find((el) => el.text() === '2')
    expect(pageTwo).toBeTruthy()
    await pageTwo!.trigger('click')
    await nextTick()
    await nextTick()

    expect(lastSearchArgs()).toMatchObject({ query: '午餐', page: 2, pageSize: 20 })
  })

  it('无匹配结果显示「命中 0 条」与空态提示', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '不存在的关键字')
    expect(wrapper.text()).toContain('命中 0 条')
    expect(wrapper.text()).toContain('无匹配结果')
  })

  it('结果只读：不渲染删除操作', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '报销')
    expect(wrapper.text()).not.toContain('删除')
  })

  describe('金额/日期筛选（issue #41）', () => {
    it('单边筛选：只填最低金额生效，金额小数元 → 分（15.5 → 1550）', async () => {
      vi.useFakeTimers()
      const wrapper = mount(SearchView)
      await nextTick()
      await minAmountInput(wrapper).setValue('15.5')
      await applyFilters(wrapper)
      expect(searchCalls().length).toBe(1)
      expect(lastSearchArgs()).toMatchObject({
        query: '',
        page: 1,
        pageSize: 20,
        amountMinCents: 1550,
        amountMaxCents: null,
        dateFrom: null,
        dateTo: null,
      })
    })

    it('单边筛选：只填最高金额生效', async () => {
      vi.useFakeTimers()
      const wrapper = mount(SearchView)
      await nextTick()
      await maxAmountInput(wrapper).setValue('20')
      await applyFilters(wrapper)
      expect(lastSearchArgs()).toMatchObject({
        query: '',
        amountMinCents: null,
        amountMaxCents: 2000,
      })
    })

    it('筛选+关键字 AND 组合时 invoke 参数正确', async () => {
      vi.useFakeTimers()
      const wrapper = mount(SearchView)
      await nextTick()
      await typeAndSearch(wrapper, '午餐')
      await minAmountInput(wrapper).setValue('15')
      await maxAmountInput(wrapper).setValue('30')
      await setDate(wrapper, 0, '2026-02-05')
      await setDate(wrapper, 1, '2026-02-20')
      await applyFilters(wrapper)
      expect(lastSearchArgs()).toMatchObject({
        query: '午餐',
        amountMinCents: 1500,
        amountMaxCents: 3000,
        dateFrom: '2026-02-05',
        dateTo: '2026-02-20',
      })
    })

    it('无关键字仅筛选可出结果', async () => {
      vi.useFakeTimers()
      const wrapper = mount(SearchView)
      await nextTick()
      await minAmountInput(wrapper).setValue('30')
      await applyFilters(wrapper)
      // 金额 ≥ 3000 分：i=20..24 共 5 条（¥30.00 ~ ¥34.00）
      expect(wrapper.text()).toContain('命中 5 条')
      expect(lastSearchArgs()).toMatchObject({ query: '', amountMinCents: 3000 })
    })

    it('日期筛选（含边界）生效：起始+结束', async () => {
      vi.useFakeTimers()
      const wrapper = mount(SearchView)
      await nextTick()
      await setDate(wrapper, 0, '2026-02-01')
      await setDate(wrapper, 1, '2026-02-03')
      await applyFilters(wrapper)
      // 2026-02-01 ~ 02-03 含边界：i=0..2 共 3 条
      expect(wrapper.text()).toContain('命中 3 条')
      expect(lastSearchArgs()).toMatchObject({
        query: '',
        dateFrom: '2026-02-01',
        dateTo: '2026-02-03',
      })
    })

    it('筛选变化同样防抖 ~300ms 触发查询', async () => {
      vi.useFakeTimers()
      const wrapper = mount(SearchView)
      await nextTick()
      await minAmountInput(wrapper).setValue('10')
      await vi.advanceTimersByTimeAsync(299)
      expect(searchCalls().length).toBe(0)
      await minAmountInput(wrapper).setValue('15')
      await vi.advanceTimersByTimeAsync(200)
      expect(searchCalls().length).toBe(0)
      await vi.advanceTimersByTimeAsync(100)
      await nextTick()
      await nextTick()
      expect(searchCalls().length).toBe(1)
      expect(lastSearchArgs()).toMatchObject({ query: '', amountMinCents: 1500 })
    })

    it('筛选激活时显示当前筛选条件，清除筛选后重置', async () => {
      vi.useFakeTimers()
      const wrapper = mount(SearchView)
      await nextTick()
      await minAmountInput(wrapper).setValue('15.5')
      await setDate(wrapper, 0, '2026-02-05')
      await applyFilters(wrapper)
      expect(wrapper.text()).toContain('已应用筛选')
      expect(wrapper.text()).toContain('最低 ¥15.5')
      expect(wrapper.text()).toContain('起始 2026-02-05')

      const clearBtn = wrapper.findAll('button').find((b) => b.text() === '清除筛选')
      expect(clearBtn).toBeTruthy()
      await clearBtn!.trigger('click')
      await applyFilters(wrapper)
      expect(wrapper.text()).not.toContain('已应用筛选')
      expect(minAmountInput(wrapper).element as HTMLInputElement).toHaveProperty('value', '')
      // 关键字也为空 → 回到占位提示
      expect(wrapper.text()).toContain('输入关键字或设置筛选开始搜索')
      expect(searchCalls().length).toBe(1) // 清除后无新查询
    })

    it('非法金额输入视为无筛选，不触发查询', async () => {
      vi.useFakeTimers()
      const wrapper = mount(SearchView)
      await nextTick()
      await minAmountInput(wrapper).setValue('abc')
      await applyFilters(wrapper)
      expect(searchCalls().length).toBe(0)
      expect(wrapper.text()).toContain('输入关键字或设置筛选开始搜索')
    })
  })
})

describe('SearchView 转账行双向账户名（issue #99）', () => {
  it('搜索结果转账行显示「转出 → 转入」双向账户名，两个名字各自可点击、各自跳转对应账户', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '转账')
    expect(wrapper.text()).toContain('命中 1 条')
    // 双向展示：转出（现金）→ 转入（银行）两个可点击账户名 + 箭头
    const links = wrapper.findAllComponents(AccountLink)
    expect(links.length).toBe(2)
    expect(links.map((l) => l.text())).toEqual(['现金', '银行'])
    expect(wrapper.text()).toContain('→')
    expect(links[0].attributes('title')).toBe('查看该账户的交易')
    expect(links[1].attributes('title')).toBe('查看该账户的交易')
    // 转出账户点击 → 跳转其过滤视图
    await links[0].find('button').trigger('click')
    expect(pushMock).toHaveBeenLastCalledWith({
      name: 'transactions',
      query: { account: 'acc-cash' },
    })
    // 转入账户点击 → 跳转其过滤视图
    await links[1].find('button').trigger('click')
    expect(pushMock).toHaveBeenLastCalledWith({
      name: 'transactions',
      query: { account: 'acc-bank' },
    })
  })

  it('搜索结果非转账行仍显示单个主账户名（可点击）', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '报销')
    // 2 条报销 expense 行，各渲染单个账户链接
    const links = wrapper.findAllComponents(AccountLink)
    expect(links.length).toBe(2)
    expect(links.map((l) => l.text())).toEqual(['现金', '现金'])
    await links[0].find('button').trigger('click')
    expect(pushMock).toHaveBeenLastCalledWith({
      name: 'transactions',
      query: { account: 'acc-cash' },
    })
  })
})
