import { describe, it, expect, vi, beforeEach, afterEach, beforeAll } from 'vitest'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { nextTick } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { NButton, NDatePicker } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import SearchView from '@/views/SearchView.vue'
import AccountLink from '@/components/AccountLink.vue'
import { applyLocale } from '@/i18n'
import { resetOverlays } from '@/composables/overlayRegistry'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import type { Account, Category, Merchant, Transaction } from '@/types'

const mockInvoke = vi.mocked(invoke)

// jsdom 无元素滚动：期间直达面板打开时 naive-ui 会 scrollTo，补空实现避免
// 打断 Vue 调度队列（QuickTimeRange 组件测试同款前提）。
beforeAll(() => {
  Element.prototype.scrollTo = () => {}
})

// AccountLink 经 useRouter 跳转（pushMock 断言导航目标，issue #99）
const pushMock = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push: pushMock }),
}))

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
// 金额随索引递增（1000 + i*100 分），日期逐日递增（2026-02-01 ~ 2026-02-25），供金额/期间筛选测试。
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

// 数据期间边界夹具（QuickTimeRange 钳制输入）：「今天」= 2026-02-10 时月档边界
// 为 [2025-12, 2026-03]——最早端来自数据 2025-12，最新端 2026-03 为最新交易期间
//（大于当前期间的抬升）。
const MOCK_RANGE = { min_date: '2025-12-15', max_date: '2026-03-01' }

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

async function applyFilters(wrapper: VueWrapper, delay = 300) {
  await vi.advanceTimersByTimeAsync(delay)
  await nextTick()
  await nextTick()
}

/** 固定「今天」= 2026-02-10（本地）：芯片快照与高亮随之确定——当月 2026-02、
 * 当季 2026Q1、当年 2026、去年 2025（报表页视图测试同款前提，期望日期全用字面量）。 */
function freezeToday() {
  vi.useFakeTimers()
  vi.setSystemTime(new Date(2026, 1, 10, 12, 0, 0))
}

/** 芯片按钮按文案定位（闭集文案唯一：全部/当月/当季/当年/去年——交易页时间维度行测试同款）。 */
const chip = (wrapper: VueWrapper, label: string) =>
  wrapper.findAllComponents(NButton).find((b) => b.text().trim() === label)!

async function clickChip(wrapper: VueWrapper, label: string) {
  await chip(wrapper, label).trigger('click')
  await flushPromises()
}

const lit = (wrapper: VueWrapper, label: string) => chip(wrapper, label).props('type') === 'primary'

/** 步进按钮按 aria-label 定位（图标按钮无文案）。 */
const stepButton = (wrapper: VueWrapper, key: 'prev' | 'next') =>
  wrapper
    .findAllComponents(NButton)
    .find((b) => b.attributes('aria-label') === (key === 'prev' ? '上一个周期' : '下一个周期'))!

async function step(wrapper: VueWrapper, key: 'prev' | 'next') {
  await stepButton(wrapper, key).trigger('click')
  await flushPromises()
}

const periodLabel = (wrapper: VueWrapper) => wrapper.find('.period-label-text').text()

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  pushMock.mockReset()
  // 参考命令桩统一走共享助手（issue #725）：币种与规范夹具等值流入，账户/分类/商户
  // 保留本文件夹具（交易夹具按 acc-cash/cat-food/mer-jd 解析名称）
  stubReferenceInvoke({
    list_accounts: mockAccounts,
    list_categories: mockCategories,
    list_insurers: [],
    list_merchants: mockMerchants,
    // 数据期间边界（QuickTimeRange 钳制输入）
    report_date_range: MOCK_RANGE,
    search_transactions: (args?: Record<string, unknown>) => {
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
    },
  })
  localStorage.clear()
  const store = useReferenceStore()
  await store.refresh()
})

afterEach(() => {
  vi.useRealTimers()
  resetOverlays()
})

describe('SearchView.vue', () => {
  it('空输入显示占位提示且不触发搜索', async () => {
    const wrapper = mount(SearchView)
    await flushPromises()
    expect(wrapper.text()).toContain('输入关键字或设置筛选开始搜索')
    expect(searchCalls().length).toBe(0)
  })

  it('i18n：切 en-US 后空态/占位即时切换英文，还原后中文逐字不变（issue #348）', async () => {
    const wrapper = mount(SearchView)
    await flushPromises()
    expect(wrapper.text()).toContain('输入关键字或设置筛选开始搜索')
    await applyLocale('en-US')
    await nextTick()
    expect(wrapper.text()).toContain('Enter a keyword or set filters to start searching')
    expect(wrapper.find('input').attributes('placeholder')).toContain('Enter keywords to search')
    // 还原，避免污染同文件其他用例（单例语言状态）
    await applyLocale('zh-CN')
    await nextTick()
    expect(wrapper.text()).toContain('输入关键字或设置筛选开始搜索')
  })

  it('时间控件仅剩快捷选择行：五芯片就位、默认「全部」点亮，无日期选择器残留（issue #526）', async () => {
    const wrapper = mount(SearchView)
    await flushPromises()
    const keywordInput = wrapper.find('input')
    expect(keywordInput.attributes('placeholder')).toContain('输入关键字')
    expect(minAmountInput(wrapper).attributes('placeholder')).toBe('最低金额（元）')
    expect(maxAmountInput(wrapper).attributes('placeholder')).toBe('最高金额（元）')
    for (const label of ['全部', '当月', '当季', '当年', '去年']) {
      expect(chip(wrapper, label)).toBeTruthy()
    }
    expect(lit(wrapper, '全部')).toBe(true)
    for (const label of ['当月', '当季', '当年', '去年']) {
      expect(lit(wrapper, label)).toBe(false)
    }
    // 两个独立日期选择器（任意起止/可单边）退役：无「起始日期/结束日期」占位残留
    const placeholders = wrapper.findAll('input').map((i) => i.attributes('placeholder'))
    expect(placeholders).not.toContain('起始日期')
    expect(placeholders).not.toContain('结束日期')
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

  describe('金额筛选（issue #41）', () => {
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

    it('筛选激活时显示当前筛选条件，清除后重置', async () => {
      vi.useFakeTimers()
      const wrapper = mount(SearchView)
      await nextTick()
      await minAmountInput(wrapper).setValue('15.5')
      await applyFilters(wrapper)
      expect(wrapper.text()).toContain('已应用筛选')
      expect(wrapper.text()).toContain('最低 ¥15.5')

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

  // 时间范围快捷选择（issue #526 / ADR-0070，消费形态三）：原两个日期选择器的
  // 交互用例就地改写为芯片交互用例（先例：报表页视图测试——真实挂载视图与快捷
  // 选择组件、mock invoke 夹具、固定「今天」、断言搜索载荷）；单边日期用例随
  // 能力退役删除。芯片换算/钳制数学的组件级单测见 QuickTimeRange.test.ts 与
  // time-period.test.ts，此处只测视图外部行为：载荷、防抖、点亮与清除。
  describe('时间范围快捷选择（issue #526 / ADR-0070）', () => {
    it('点「当月」写入双端有界快照并防抖自动搜索，芯片点亮切换', async () => {
      freezeToday()
      const wrapper = mount(SearchView)
      await nextTick()
      await clickChip(wrapper, '当月')
      // 沿用既有防抖：300ms 到点才触发
      expect(searchCalls().length).toBe(0)
      await vi.advanceTimersByTimeAsync(299)
      expect(searchCalls().length).toBe(0)
      await vi.advanceTimersByTimeAsync(1)
      await nextTick()
      await nextTick()
      expect(searchCalls().length).toBe(1)
      expect(lastSearchArgs()).toMatchObject({
        dateFrom: '2026-02-01',
        dateTo: '2026-02-28',
      })
      // 2026-02-01 ~ 02-28 含边界：25 条日递增交易 + 02-26 转账共 26 条（03-01 不在内）
      expect(wrapper.text()).toContain('命中 26 条')
      expect(lit(wrapper, '当月')).toBe(true)
      expect(lit(wrapper, '全部')).toBe(false)
    })

    it('五枚日期芯片各自写入对应自然周期快照（当季/当年/去年），命中数随期间变化', async () => {
      freezeToday()
      const wrapper = mount(SearchView)
      await nextTick()
      // 当季 2026-01-01 ~ 03-31：含全部夹具 27 条
      await clickChip(wrapper, '当季')
      await applyFilters(wrapper)
      expect(lastSearchArgs()).toMatchObject({ dateFrom: '2026-01-01', dateTo: '2026-03-31' })
      expect(wrapper.text()).toContain('命中 27 条')
      expect(lit(wrapper, '当季')).toBe(true)
      // 当年 2026 全年：同为 27 条
      await clickChip(wrapper, '当年')
      await applyFilters(wrapper)
      expect(lastSearchArgs()).toMatchObject({ dateFrom: '2026-01-01', dateTo: '2026-12-31' })
      expect(wrapper.text()).toContain('命中 27 条')
      // 去年 2025 全年：无数据，命中 0 条（有界空区间是诚实结果）
      await clickChip(wrapper, '去年')
      await applyFilters(wrapper)
      expect(lastSearchArgs()).toMatchObject({ dateFrom: '2025-01-01', dateTo: '2025-12-31' })
      expect(wrapper.text()).toContain('命中 0 条')
      expect(lit(wrapper, '去年')).toBe(true)
    })

    it('点「全部」一键清除日期条件：双空载荷，回默认态（issue #526）', async () => {
      freezeToday()
      const wrapper = mount(SearchView)
      await nextTick()
      await typeAndSearch(wrapper, '午餐')
      await clickChip(wrapper, '当月')
      await applyFilters(wrapper)
      expect(lastSearchArgs()).toMatchObject({ dateFrom: '2026-02-01', dateTo: '2026-02-28' })
      await clickChip(wrapper, '全部')
      await applyFilters(wrapper)
      expect(lastSearchArgs()).toMatchObject({ query: '午餐', dateFrom: null, dateTo: null })
      expect(wrapper.text()).toContain('命中 23 条')
      expect(lit(wrapper, '全部')).toBe(true)
      expect(lit(wrapper, '当月')).toBe(false)
    })

    it('重复点同一芯片不重复搜索（同值守卫：快照未变不动作）', async () => {
      freezeToday()
      const wrapper = mount(SearchView)
      await nextTick()
      await clickChip(wrapper, '当月')
      await applyFilters(wrapper)
      expect(searchCalls().length).toBe(1)
      await clickChip(wrapper, '当月')
      await applyFilters(wrapper)
      expect(searchCalls().length).toBe(1)
    })

    it('关键字＋金额＋时间范围 AND 组合：载荷日期双端有界（原日期选择器组合用例改造）', async () => {
      freezeToday()
      const wrapper = mount(SearchView)
      await nextTick()
      await typeAndSearch(wrapper, '午餐')
      await minAmountInput(wrapper).setValue('15')
      await maxAmountInput(wrapper).setValue('30')
      await clickChip(wrapper, '当月')
      await applyFilters(wrapper)
      expect(lastSearchArgs()).toMatchObject({
        query: '午餐',
        amountMinCents: 1500,
        amountMaxCents: 3000,
        dateFrom: '2026-02-01',
        dateTo: '2026-02-28',
      })
    })

    it('「清除筛选」把日期条件一并清回「全部」（双空）', async () => {
      freezeToday()
      const wrapper = mount(SearchView)
      await nextTick()
      await minAmountInput(wrapper).setValue('15.5')
      await clickChip(wrapper, '当月')
      await applyFilters(wrapper)
      // 金额与芯片变更防抖合并为一次搜索
      expect(searchCalls().length).toBe(1)
      expect(wrapper.text()).toContain('已应用筛选')
      expect(wrapper.text()).toContain('最低 ¥15.5')
      expect(wrapper.text()).toContain('起始 2026-02-01')
      expect(wrapper.text()).toContain('结束 2026-02-28')

      const clearBtn = wrapper.findAll('button').find((b) => b.text() === '清除筛选')
      expect(clearBtn).toBeTruthy()
      await clearBtn!.trigger('click')
      await applyFilters(wrapper)
      expect(wrapper.text()).not.toContain('已应用筛选')
      expect(minAmountInput(wrapper).element as HTMLInputElement).toHaveProperty('value', '')
      // 日期条件清回「全部」：芯片回默认点亮、载荷双空
      expect(lit(wrapper, '全部')).toBe(true)
      expect(lit(wrapper, '当月')).toBe(false)
      // 关键字也为空 → 回到占位提示
      expect(wrapper.text()).toContain('输入关键字或设置筛选开始搜索')
      expect(searchCalls().length).toBe(1) // 清除后无新查询
    })

    it('「全部」态步进器置灰、期间标签占位，面板仍可开', async () => {
      freezeToday()
      const wrapper = mount(SearchView)
      await flushPromises()
      expect(stepButton(wrapper, 'prev').props('disabled')).toBe(true)
      expect(stepButton(wrapper, 'next').props('disabled')).toBe(true)
      expect(periodLabel(wrapper)).toBe('选择期间')
      // 步进不可达，但期间标签仍可打开直达面板（aria-expanded 随开合翻转）
      const trigger = wrapper.find('.period-label')
      await trigger.trigger('click')
      await nextTick()
      expect(wrapper.find('button[aria-haspopup="dialog"]').attributes('aria-expanded')).toBe('true')
    })

    it('期间直达面板钳制于数据期间边界：全部态默认月档，界外月份不可选', async () => {
      freezeToday()
      const wrapper = mount(SearchView)
      await flushPromises()
      const picker = wrapper.findComponent(NDatePicker)
      expect(picker.props('type')).toBe('month')
      const isDisabled = picker.props('isDateDisabled') as (
        timestamp: number,
        detail: unknown,
      ) => boolean
      // 月档边界 [2025-12, 2026-03]：界外不可选、界内（含两端）可选
      // （naive-ui 的月份 detail.month 为 0 起）
      expect(isDisabled(0, { type: 'month', year: 2025, month: 10 })).toBe(true)
      expect(isDisabled(0, { type: 'month', year: 2025, month: 11 })).toBe(false)
      expect(isDisabled(0, { type: 'month', year: 2026, month: 2 })).toBe(false)
      expect(isDisabled(0, { type: 'month', year: 2026, month: 3 })).toBe(true)
    })

    it('期间直达面板点选写入精确期间快照：防抖搜索载荷双端有界（视图接缝端到端）', async () => {
      freezeToday()
      const wrapper = mount(SearchView)
      await flushPromises()
      // 经直达面板选 2025-12（界内历史月份）：面板载体为隐形 NDatePicker，
      // 沿旧日期选择器用例先例在组件缝 emit 点选结果（避免 fake timers 下开面板）
      const picker = wrapper.findComponent(NDatePicker)
      picker.vm.$emit('update:value', new Date(2025, 11, 15, 12).getTime())
      await flushPromises()
      expect(searchCalls().length).toBe(0) // 沿用既有防抖，未到点不搜索
      await applyFilters(wrapper)
      expect(searchCalls().length).toBe(1)
      expect(lastSearchArgs()).toMatchObject({ dateFrom: '2025-12-01', dateTo: '2025-12-31' })
      // 历史期间非预设 → 芯片全灭；步进游标落在月档下界（prev 置灰）
      for (const label of ['全部', '当月', '当季', '当年', '去年']) {
        expect(lit(wrapper, label)).toBe(false)
      }
      expect(periodLabel(wrapper)).toBe('2025年12月')
      expect(stepButton(wrapper, 'prev').props('disabled')).toBe(true)
    })

    it('期间步进写有界快照并受数据期间边界钳制（边界外置灰）', async () => {
      freezeToday()
      const wrapper = mount(SearchView)
      await flushPromises()
      await clickChip(wrapper, '当月')
      await applyFilters(wrapper)
      expect(periodLabel(wrapper)).toBe('2026年2月')
      // 上界：当月 → next 到 2026-03（最新交易期间），再 next 置灰
      expect(stepButton(wrapper, 'next').props('disabled')).toBe(false)
      await step(wrapper, 'next')
      await applyFilters(wrapper)
      expect(lastSearchArgs()).toMatchObject({ dateFrom: '2026-03-01', dateTo: '2026-03-31' })
      expect(periodLabel(wrapper)).toBe('2026年3月')
      // 历史期间不是任何预设定义 → 芯片全灭，列表快照不漂移
      for (const label of ['全部', '当月', '当季', '当年', '去年']) {
        expect(lit(wrapper, label)).toBe(false)
      }
      expect(stepButton(wrapper, 'next').props('disabled')).toBe(true)
      // 下界：连步回 2025-12（最早交易期间），再 prev 置灰
      await step(wrapper, 'prev')
      await applyFilters(wrapper)
      await step(wrapper, 'prev')
      await applyFilters(wrapper)
      await step(wrapper, 'prev')
      await applyFilters(wrapper)
      expect(lastSearchArgs()).toMatchObject({ dateFrom: '2025-12-01', dateTo: '2025-12-31' })
      expect(periodLabel(wrapper)).toBe('2025年12月')
      expect(stepButton(wrapper, 'prev').props('disabled')).toBe(true)
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
