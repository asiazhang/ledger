import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { NInput, NSelect, NTreeSelect, NDatePicker, NPopconfirm } from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import SubscriptionsView from '@/views/SubscriptionsView.vue'
import type {
  Account,
  Category,
  Currency,
  ScheduledTransaction,
  ScheduledTransactionDetail,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
  SubscriptionSpendOverview,
} from '@/types'

const mockInvoke = vi.mocked(invoke)

enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1',
    name: '招商银行',
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
]

const mockCategories: Category[] = [
  {
    id: 'cat-1',
    name: '订阅服务',
    kind: 'expense',
    parent_id: null,
    icon: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

/** 订阅计划工厂：core.kind 固定 subscription，其余可覆写 */
function makePlan(
  partial: Partial<ScheduledTransaction> & { id: string },
): ScheduledTransactionWithExt {
  const core: ScheduledTransaction = {
    kind: 'subscription',
    status: 'active',
    account_id: 'acc-1',
    category_id: 'cat-1',
    amount_cents: 1500,
    currency_code: 'CNY',
    recurrence_type: 'monthly',
    recurrence_interval: 1,
    recurrence_day: null,
    start_date: '2026-01-01',
    note: '视频会员',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...partial,
  }
  return {
    core,
    counterparty: null,
    total_amount_cents: null,
    total_occurrences: null,
    to_account_id: null,
  }
}

function makeOccurrence(
  partial: Partial<ScheduledTransactionOccurrence> & { id: string },
): ScheduledTransactionOccurrence {
  return {
    scheduled_transaction_id: 'unknown',
    scheduled_date: '2026-03-01',
    status: 'pending',
    transaction_id: null,
    amount_cents: 1500,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...partial,
  }
}

function makeDetail(
  plan: ScheduledTransactionWithExt,
  pending_occurrences: ScheduledTransactionOccurrence[],
): ScheduledTransactionDetail {
  return {
    core: plan.core,
    extension: {
      scheduled_transaction_id: plan.core.id,
      counterparty: plan.counterparty,
    },
    pending_occurrences,
    completed_occurrences: 0,
  }
}

// —— invoke mock：可变数据源，状态操作后重载读得到最新值 ——
let mockPlans: ScheduledTransactionWithExt[] = []
const mockDetails = new Map<string, ScheduledTransactionDetail>()

/** 订阅花费总览 fixture（issue #160）：面板挂载即拉取，默认空数据 */
const emptySpendOverview: SubscriptionSpendOverview = {
  native_currency: 'CNY',
  this_month_native_cents: 0,
  this_year_native_cents: 0,
  months: [],
  rows: [],
}
let mockSpendOverview: SubscriptionSpendOverview = emptySpendOverview

function baseInvoke() {
  mockInvoke.mockImplementation(((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve(mockCategories)
    if (cmd === 'subscription_spend_overview') return Promise.resolve(mockSpendOverview)
    if (cmd === 'list_scheduled_transactions') return Promise.resolve(mockPlans)
    if (cmd === 'get_scheduled_transaction_detail') {
      const detail = mockDetails.get(String(args?.id))
      return detail ? Promise.resolve(detail) : Promise.reject(new Error('无此计划详情'))
    }
    if (cmd === 'create_scheduled_transaction') {
      const input = args?.input as { kind: string; note: string | null }
      const id = `new-${input.kind}-${input.note ?? ''}`
      const plan = makePlan({ id, note: input.note ?? null })
      mockPlans = [...mockPlans, plan]
      mockDetails.set(id, makeDetail(plan, []))
      return Promise.resolve(id)
    }
    if (cmd === 'update_scheduled_transaction_status') {
      const { id, new_status } = args as { id: string; new_status: string }
      mockPlans = mockPlans.map((p) =>
        p.core.id === id ? { ...p, core: { ...p.core, status: new_status } } : p,
      )
      const detail = mockDetails.get(id)
      if (detail) {
        mockDetails.set(id, { ...detail, core: { ...detail.core, status: new_status } })
      }
      return Promise.resolve()
    }
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  }) as typeof invoke)
}

async function mountView() {
  const wrapper = mount(SubscriptionsView)
  await flushPromises()
  return wrapper
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockPlans = []
  mockDetails.clear()
  mockSpendOverview = emptySpendOverview
  baseInvoke()
  const store = useReferenceStore()
  await store.ensureFresh()
})

describe('SubscriptionsView 订阅清单（issue #159）', () => {
  it('默认只显示进行中（active）的订阅', async () => {
    mockPlans = [
      makePlan({ id: 'a1', note: '进行中订阅' }),
      makePlan({ id: 'p1', note: '已暂停订阅', status: 'paused' }),
      makePlan({ id: 'c1', note: '已取消订阅', status: 'cancelled' }),
    ]
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('进行中订阅')
    expect(wrapper.text()).not.toContain('已暂停订阅')
    expect(wrapper.text()).not.toContain('已取消订阅')
  })

  it('切换过滤查看已暂停 / 已取消', async () => {
    mockPlans = [
      makePlan({ id: 'a1', note: '进行中订阅' }),
      makePlan({ id: 'p1', note: '已暂停订阅', status: 'paused' }),
      makePlan({ id: 'c1', note: '已取消订阅', status: 'cancelled' }),
    ]
    const wrapper = await mountView()
    await wrapper.find('[data-testid="filter-paused"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已暂停订阅')
    expect(wrapper.text()).not.toContain('进行中订阅')

    await wrapper.find('[data-testid="filter-cancelled"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已取消订阅')
    expect(wrapper.text()).not.toContain('已暂停订阅')
  })

  it('只展示订阅计划，分期 / 定时转账不出现', async () => {
    mockPlans = [
      makePlan({ id: 'a1', note: '视频会员' }),
      makePlan({ id: 'i1', note: '某分期', kind: 'installment' }),
      makePlan({ id: 't1', note: '某定时转账', kind: 'scheduled_transfer' }),
    ]
    mockDetails.set('a1', makeDetail(mockPlans[0], []))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('视频会员')
    expect(wrapper.text()).not.toContain('某分期')
    expect(wrapper.text()).not.toContain('某定时转账')
  })

  it('每行显示下期扣款日与金额（取最早 pending 期次）', async () => {
    const plan = makePlan({ id: 'a1', amount_cents: 1500 })
    mockPlans = [plan]
    mockDetails.set(
      'a1',
      makeDetail(plan, [
        makeOccurrence({ id: 'o2', scheduled_date: '2026-04-01' }),
        makeOccurrence({ id: 'o1', scheduled_date: '2026-03-01' }),
      ]),
    )
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('2026-03-01')
    expect(wrapper.text()).toContain('¥15')
    expect(wrapper.text()).not.toContain('2026-04-01')
  })

  it('无 pending 期次（窗口外/已取消）时下期扣款显示 — 占位，不推算日期', async () => {
    const plan = makePlan({ id: 'a1' })
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    const cell = wrapper.find('[data-testid="next-charge-a1"]')
    expect(cell.text()).toBe('—')
    // 不推算日期：占位格里不出现任何日期形串
    expect(cell.text()).not.toMatch(/\d{4}-\d{2}-\d{2}/)
  })

  it('详情命令失败时显示加载失败，不与「无 pending」混淆', async () => {
    const plan = makePlan({ id: 'a1' })
    mockPlans = [plan]
    // 不注册 a1 的详情：get_scheduled_transaction_detail 将 reject
    const wrapper = await mountView()
    expect(wrapper.find('[data-testid="next-charge-a1"]').text()).toBe('加载失败')
  })

  it('金额与周期按原始币种与规则展示', async () => {
    const plan = makePlan({ id: 'a1', amount_cents: 9900, recurrence_interval: 3 })
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('¥99')
    expect(wrapper.text()).toContain('每3月')
  })
})

describe('SubscriptionsView 新建订阅（issue #159）', () => {
  it('填表创建走既有创建命令，金额转分、kind=subscription', async () => {
    const wrapper = await mountView()
    const noteInput = wrapper.find('[data-testid="sub-note"] input')
    await noteInput.setValue('音乐订阅')
    await noteInput.trigger('input')
    const amountInput = wrapper.find('[data-testid="sub-amount"] input')
    await amountInput.setValue('25')
    await amountInput.trigger('input')
    // 账户 / 分类 / 周期：经组件 emit 设置
    wrapper.findComponent(NSelect).vm.$emit('update:value', 'acc-1')
    wrapper.findComponent(NTreeSelect).vm.$emit('update:value', 'cat-1')
    wrapper
      .findComponent(NDatePicker)
      .vm.$emit('update:formatted-value', '2026-02-15')
    await flushPromises()

    const createBtn = wrapper.find('[data-testid="sub-create"]')
    await createBtn.trigger('click')
    await flushPromises()

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_scheduled_transaction')
    expect(call).toBeDefined()
    expect(call![1]).toEqual({
      input: {
        kind: 'subscription',
        account_id: 'acc-1',
        category_id: 'cat-1',
        amount_cents: 2500,
        currency_code: 'CNY',
        recurrence_type: 'monthly',
        recurrence_interval: 1,
        recurrence_day: null,
        start_date: '2026-02-15',
        note: '音乐订阅',
      },
    })
  })

  it('未选账户时不提交创建', async () => {
    const wrapper = await mountView()
    const amountInput = wrapper.find('[data-testid="sub-amount"] input')
    await amountInput.setValue('25')
    await amountInput.trigger('input')
    await wrapper.find('[data-testid="sub-create"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_scheduled_transaction'),
    ).toBe(false)
  })

  it('创建成功后刷新清单，新订阅出现在列表', async () => {
    const wrapper = await mountView()
    const noteInput = wrapper.find('[data-testid="sub-note"] input')
    await noteInput.setValue('云存储')
    await noteInput.trigger('input')
    const amountInput = wrapper.find('[data-testid="sub-amount"] input')
    await amountInput.setValue('6')
    await amountInput.trigger('input')
    wrapper.findComponent(NSelect).vm.$emit('update:value', 'acc-1')
    await flushPromises()
    await wrapper.find('[data-testid="sub-create"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('云存储')
  })
})

describe('SubscriptionsView 状态操作（issue #159）', () => {
  it('进行中的订阅可暂停（走既有状态命令）', async () => {
    const plan = makePlan({ id: 'a1' })
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    await wrapper.find('[data-testid="op-pause-a1"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { id: string; input: { new_status: string } }).input.new_status === 'paused',
      ),
    ).toBe(true)
  })

  it('已暂停的订阅可恢复', async () => {
    const plan = makePlan({ id: 'p1', status: 'paused' })
    mockPlans = [plan]
    mockDetails.set('p1', makeDetail(plan, []))
    const wrapper = await mountView()
    await wrapper.find('[data-testid="filter-paused"]').trigger('click')
    await flushPromises()
    await wrapper.find('[data-testid="op-resume-p1"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { id: string; input: { new_status: string } }).input.new_status === 'active',
      ),
    ).toBe(true)
  })

  it('取消需二次确认（NPopconfirm），确认后走状态命令', async () => {
    const plan = makePlan({ id: 'a1' })
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    // 打开 Popconfirm
    await wrapper
      .findComponent(NPopconfirm)
      .find('[data-testid="op-cancel-a1"]')
      .trigger('click')
    await flushPromises()
    const positive = document.body.querySelector('.n-popconfirm .n-button--primary-type')
    expect(positive).not.toBeNull()
    ;(positive as HTMLButtonElement).click()
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { id: string; input: { new_status: string } }).input.new_status ===
            'cancelled',
      ),
    ).toBe(true)
  })

  it('已取消的订阅不再提供状态操作', async () => {
    const plan = makePlan({ id: 'c1', status: 'cancelled', note: '已取消订阅' })
    mockPlans = [plan]
    mockDetails.set('c1', makeDetail(plan, []))
    const wrapper = await mountView()
    await wrapper.find('[data-testid="filter-cancelled"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已取消订阅')
    expect(wrapper.find('[data-testid="op-pause-c1"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="op-resume-c1"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="op-cancel-c1"]').exists()).toBe(false)
  })
})
