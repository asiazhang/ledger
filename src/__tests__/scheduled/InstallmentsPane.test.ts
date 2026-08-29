import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import {
  NInput,
  NModal,
  NSelect,
  NTreeSelect,
  NDatePicker,
  NPopconfirm,
  NProgress,
} from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import InstallmentsPane from '@/components/scheduled/InstallmentsPane.vue'
import type {
  Account,
  Category,
  Currency,
  InstallmentPlan,
  ScheduledTransaction,
  ScheduledTransactionDetail,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
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
    name: '数码分期',
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

/** 分期计划工厂：core.kind 固定 installment；扩展字段携带总额与期数。 */
function makePlan(
  partial: Partial<ScheduledTransaction> & { id: string },
  ext: { total_amount_cents: number; total_occurrences: number },
): ScheduledTransactionWithExt {
  const core: ScheduledTransaction = {
    kind: 'installment',
    status: 'active',
    account_id: 'acc-1',
    category_id: 'cat-1',
    amount_cents: Math.floor(ext.total_amount_cents / ext.total_occurrences),
    currency_code: 'CNY',
    recurrence_type: 'monthly',
    recurrence_interval: 1,
    recurrence_day: null,
    start_date: '2026-01-01',
    note: '手机分期',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...partial,
  }
  return {
    core,
    merchant_id: null,
    total_amount_cents: ext.total_amount_cents,
    total_occurrences: ext.total_occurrences,
    to_account_id: null,
  }
}

function makeDetail(
  plan: ScheduledTransactionWithExt,
  completed: { count: number; amount: number },
): ScheduledTransactionDetail {
  const extension: InstallmentPlan = {
    scheduled_transaction_id: plan.core.id,
    merchant_id: null,
    total_amount_cents: plan.total_amount_cents ?? 0,
    total_occurrences: plan.total_occurrences ?? 0,
  }
  return {
    core: plan.core,
    extension,
    pending_occurrences: [],
    completed_occurrences: completed.count,
    completed_amount_cents: completed.amount,
  }
}

// —— invoke mock：可变数据源，状态操作后重载读得到最新值 ——
let mockPlans: ScheduledTransactionWithExt[] = []
const mockDetails = new Map<string, ScheduledTransactionDetail>()

function baseInvoke() {
  mockInvoke.mockImplementation(((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve(mockCategories)
    if (cmd === 'list_merchants') return Promise.resolve([])
    if (cmd === 'list_scheduled_transactions') return Promise.resolve(mockPlans)
    if (cmd === 'get_scheduled_transaction_detail') {
      const detail = mockDetails.get(String(args?.id))
      return detail ? Promise.resolve(detail) : Promise.reject(new Error('无此计划详情'))
    }
    if (cmd === 'create_scheduled_transaction') {
      const input = args?.input as { kind: string; note: string | null }
      const id = `new-${input.kind}-${input.note ?? ''}`
      const plan = makePlan(
        { id, note: input.note ?? null },
        {
          total_amount_cents:
            (args?.input as { total_amount_cents: number }).total_amount_cents ?? 0,
          total_occurrences: (args?.input as { total_occurrences: number }).total_occurrences ?? 1,
        },
      )
      mockPlans = [...mockPlans, plan]
      mockDetails.set(id, makeDetail(plan, { count: 0, amount: 0 }))
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

/** 定位弹窗表单内输入框：NModal teleport 到 body，需经 findComponent 锚定。 */
function findInput(wrapper: ReturnType<typeof mount>, testid: string) {
  return wrapper.findComponent(`[data-testid="${testid}"]`).find('input')
}

/** 弹窗内普通元素（非组件）经 document.body 查询：NModal teleport 到 body。 */
function modalText(testid: string) {
  return document.body.querySelector(`[data-testid="${testid}"]`)?.textContent ?? ''
}

async function mountView() {
  const wrapper = mount(InstallmentsPane)
  await flushPromises()
  return wrapper
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockPlans = []
  mockDetails.clear()
  baseInvoke()
  const store = useReferenceStore()
  await store.ensureFresh()
})

describe('InstallmentsPane 分期清单（issue #204）', () => {
  it('只展示分期计划，订阅 / 定时转账不出现', async () => {
    const inst = makePlan(
      { id: 'i1', note: '手机分期' },
      { total_amount_cents: 120000, total_occurrences: 12 },
    )
    mockPlans = [
      inst,
      makePlan(
        { id: 's1', note: '某订阅', kind: 'subscription' },
        { total_amount_cents: 0, total_occurrences: 0 },
      ),
      makePlan(
        { id: 't1', note: '某定时转账', kind: 'scheduled_transfer' },
        { total_amount_cents: 0, total_occurrences: 0 },
      ),
    ]
    mockDetails.set('i1', makeDetail(inst, { count: 3, amount: 30000 }))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('手机分期')
    expect(wrapper.text()).not.toContain('某订阅')
    expect(wrapper.text()).not.toContain('某定时转账')
  })

  it('进度格显示进度条 + 已还金额/总额 · X/N 期（实时汇总）', async () => {
    const inst = makePlan(
      { id: 'i1', note: '手机分期' },
      { total_amount_cents: 120000, total_occurrences: 12 },
    )
    mockPlans = [inst]
    mockDetails.set('i1', makeDetail(inst, { count: 3, amount: 30000 }))
    const wrapper = await mountView()
    const cell = wrapper.find('[data-testid="inst-progress-i1"]')
    expect(cell.text()).toContain('¥300')
    expect(cell.text()).toContain('¥1200')
    expect(cell.text()).toContain('3/12 期')
    const progress = cell.findComponent(NProgress)
    expect(progress.exists()).toBe(true)
    expect(progress.props('percentage')).toBe(25)
  })

  it('已完成金额来自详情命令的 completed_amount_cents，不由前端推算', async () => {
    // 1200 分 12 期每期应为 100，但已完成汇总给 150（模拟失败重试等真实历史）：
    // 显示以汇总为准，不用 total/occurrences 推算
    const inst = makePlan(
      { id: 'i1', note: '手机分期' },
      { total_amount_cents: 1200, total_occurrences: 12 },
    )
    mockPlans = [inst]
    mockDetails.set('i1', makeDetail(inst, { count: 1, amount: 150 }))
    const wrapper = await mountView()
    expect(wrapper.find('[data-testid="inst-progress-i1"]').text()).toContain('¥1.5')
  })

  it('默认只显示进行中（active）的分期，可切换过滤', async () => {
    const a1 = makePlan(
      { id: 'a1', note: '进行中分期' },
      { total_amount_cents: 1200, total_occurrences: 12 },
    )
    const p1 = makePlan(
      { id: 'p1', note: '已暂停分期', status: 'paused' },
      { total_amount_cents: 600, total_occurrences: 6 },
    )
    mockPlans = [a1, p1]
    mockDetails.set('a1', makeDetail(a1, { count: 0, amount: 0 }))
    mockDetails.set('p1', makeDetail(p1, { count: 0, amount: 0 }))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('进行中分期')
    expect(wrapper.text()).not.toContain('已暂停分期')
    await wrapper.find('[data-testid="filter-paused"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已暂停分期')
    expect(wrapper.text()).not.toContain('进行中分期')
  })
})

describe('InstallmentsPane 新建分期（issue #204）', () => {
  /** 点击「新建分期」打开模态对话框。 */
  async function openCreateModal(wrapper: ReturnType<typeof mount>) {
    await wrapper.find('[data-testid="inst-create-open"]').trigger('click')
    await flushPromises()
  }

  it('初始无弹窗，点击按钮打开「新建分期」模态对话框', async () => {
    const wrapper = await mountView()
    const modal = wrapper.findComponent(NModal)
    expect(modal.props('show')).toBe(false)
    await openCreateModal(wrapper)
    expect(modal.props('show')).toBe(true)
    expect(modal.props('title')).toBe('新建分期')
  })

  it('弹窗不出现商户与「每月几号」字段（#204 边界）', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    const modal = wrapper.findComponent(NModal)
    expect(modal.text()).not.toContain('商户')
    expect(modal.text()).not.toContain('几号')
  })

  it('填总额与期数实时预览每期金额与末期（含尾差）', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    // 未填时无预览
    expect(modalText('inst-preview')).toBe('')
    await findInput(wrapper, 'inst-total').setValue('1')
    await findInput(wrapper, 'inst-total').trigger('input')
    wrapper
      .findComponent('[data-testid="inst-periods"]')
      .vm.$emit('update:value', 3)
    await flushPromises()
    const preview = modalText('inst-preview')
    expect(preview).toContain('¥0.33')
    expect(preview).toContain('¥0.34')
    expect(preview).toContain('尾差')
  })

  it('整除时末期与每期相等，预览不提尾差', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    await findInput(wrapper, 'inst-total').setValue('1200')
    await findInput(wrapper, 'inst-total').trigger('input')
    wrapper
      .findComponent('[data-testid="inst-periods"]')
      .vm.$emit('update:value', 12)
    await flushPromises()
    const preview = modalText('inst-preview')
    expect(preview).toContain('¥100')
    expect(preview).not.toContain('尾差')
  })

  it('提交走创建命令：amount_cents 为每期 floor 口径，携带总额与期数', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    await findInput(wrapper, 'inst-note').setValue('手机分期')
    await findInput(wrapper, 'inst-note').trigger('input')
    // 总额 1000 元 = 100000 分，分 12 期：floor(100000/12)=8333，尾差 7 分进末期
    await findInput(wrapper, 'inst-total').setValue('1000')
    await findInput(wrapper, 'inst-total').trigger('input')
    wrapper
      .findComponent('[data-testid="inst-periods"]')
      .vm.$emit('update:value', 12)
    // 账户 / 分类 / 开始日：经组件 emit 设置
    wrapper.findComponent('[data-testid="inst-account"]').vm.$emit('update:value', 'acc-1')
    wrapper.findComponent(NTreeSelect).vm.$emit('update:value', 'cat-1')
    wrapper
      .findComponent(NDatePicker)
      .vm.$emit('update:formatted-value', '2026-02-15')
    await flushPromises()

    await wrapper.findComponent('[data-testid="inst-create"]').trigger('click')
    await flushPromises()

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_scheduled_transaction')
    expect(call).toBeDefined()
    expect(call![1]).toEqual({
      input: {
        kind: 'installment',
        account_id: 'acc-1',
        category_id: 'cat-1',
        merchant_id: null,
        amount_cents: 8333,
        total_amount_cents: 100000,
        total_occurrences: 12,
        currency_code: 'CNY',
        recurrence_type: 'monthly',
        recurrence_interval: 1,
        recurrence_day: null,
        start_date: '2026-02-15',
        note: '手机分期',
      },
    })
  })

  it('未填总额或期数时不提交', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    wrapper.findComponent('[data-testid="inst-account"]').vm.$emit('update:value', 'acc-1')
    await flushPromises()
    // 只填期数不填总额
    wrapper
      .findComponent('[data-testid="inst-periods"]')
      .vm.$emit('update:value', 12)
    await flushPromises()
    await wrapper.findComponent('[data-testid="inst-create"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_scheduled_transaction'),
    ).toBe(false)
  })

  it('总额低于期数（每期不足 1 分）时不提交', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    await findInput(wrapper, 'inst-total').setValue('0.02')
    await findInput(wrapper, 'inst-total').trigger('input')
    wrapper
      .findComponent('[data-testid="inst-periods"]')
      .vm.$emit('update:value', 3)
    wrapper.findComponent('[data-testid="inst-account"]').vm.$emit('update:value', 'acc-1')
    await flushPromises()
    await wrapper.findComponent('[data-testid="inst-create"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_scheduled_transaction'),
    ).toBe(false)
  })

  it('创建成功后关闭弹窗并刷新清单，新分期出现在列表', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    await findInput(wrapper, 'inst-note').setValue('手机分期')
    await findInput(wrapper, 'inst-note').trigger('input')
    await findInput(wrapper, 'inst-total').setValue('1200')
    await findInput(wrapper, 'inst-total').trigger('input')
    wrapper
      .findComponent('[data-testid="inst-periods"]')
      .vm.$emit('update:value', 12)
    wrapper.findComponent('[data-testid="inst-account"]').vm.$emit('update:value', 'acc-1')
    await flushPromises()
    await wrapper.findComponent('[data-testid="inst-create"]').trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    expect(wrapper.text()).toContain('手机分期')
  })
})

describe('InstallmentsPane 状态操作（issue #204）', () => {
  it('进行中的分期可暂停（走既有状态命令）', async () => {
    const plan = makePlan(
      { id: 'a1' },
      { total_amount_cents: 1200, total_occurrences: 12 },
    )
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, { count: 0, amount: 0 }))
    const wrapper = await mountView()
    await wrapper.find('[data-testid="op-pause-a1"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { input: { new_status: string } }).input.new_status === 'paused',
      ),
    ).toBe(true)
  })

  it('已暂停的分期可恢复', async () => {
    const plan = makePlan(
      { id: 'p1', status: 'paused' },
      { total_amount_cents: 1200, total_occurrences: 12 },
    )
    mockPlans = [plan]
    mockDetails.set('p1', makeDetail(plan, { count: 0, amount: 0 }))
    const wrapper = await mountView()
    await wrapper.find('[data-testid="filter-paused"]').trigger('click')
    await flushPromises()
    await wrapper.find('[data-testid="op-resume-p1"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { input: { new_status: string } }).input.new_status === 'active',
      ),
    ).toBe(true)
  })

  it('取消需二次确认（NPopconfirm），说明历史保留，确认后走状态命令', async () => {
    const plan = makePlan(
      { id: 'a1' },
      { total_amount_cents: 1200, total_occurrences: 12 },
    )
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, { count: 0, amount: 0 }))
    const wrapper = await mountView()
    await wrapper
      .findComponent(NPopconfirm)
      .find('[data-testid="op-cancel-a1"]')
      .trigger('click')
    await flushPromises()
    // 确认文案说明历史保留（ADR-0024：取消不删已生成交易）
    expect(document.body.querySelector('.n-popconfirm')?.textContent).toContain('保留')
    const positive = document.body.querySelector('.n-popconfirm .n-button--primary-type')
    expect(positive).not.toBeNull()
    ;(positive as HTMLButtonElement).click()
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { input: { new_status: string } }).input.new_status === 'cancelled',
      ),
    ).toBe(true)
  })

  it('已取消的分期不再提供状态操作', async () => {
    const plan = makePlan(
      { id: 'c1', status: 'cancelled', note: '已取消分期' },
      { total_amount_cents: 1200, total_occurrences: 12 },
    )
    mockPlans = [plan]
    mockDetails.set('c1', makeDetail(plan, { count: 0, amount: 0 }))
    const wrapper = await mountView()
    await wrapper.find('[data-testid="filter-cancelled"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已取消分期')
    expect(wrapper.find('[data-testid="op-pause-c1"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="op-resume-c1"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="op-cancel-c1"]').exists()).toBe(false)
  })
})
