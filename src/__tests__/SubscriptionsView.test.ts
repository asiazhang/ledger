import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import {
  NInput,
  NModal,
  NSelect,
  NTreeSelect,
  NDatePicker,
  NInputNumber,
  NPopconfirm,
} from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import SubscriptionsView from '@/views/SubscriptionsView.vue'
import type {
  Account,
  Category,
  Currency,
  Merchant,
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

const mockMerchants: Merchant[] = [
  {
    id: 'mer-1',
    name: '视频平台',
    icon: null,
    color: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

/** 订阅计划工厂：core.kind 固定 subscription，其余可覆写；merchant_id 为扩展字段。 */
function makePlan(
  partial: Partial<ScheduledTransaction> & { id: string },
  merchant_id: string | null = null,
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
    merchant_id,
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
      merchant_id: plan.merchant_id,
    },
    pending_occurrences,
    completed_occurrences: 0,
  }
}

// —— invoke mock：可变数据源，状态操作后重载读得到最新值 ——
let mockPlans: ScheduledTransactionWithExt[] = []
const mockDetails = new Map<string, ScheduledTransactionDetail>()
/** 订阅编辑失败开关（issue #162 拒绝路径测试用） */
let failSubscriptionUpdate = false
/** 商户字典 fixture（issue #190）：新建弹窗补全与列表商户列共用 */
let mockMerchantsState: Merchant[] = mockMerchants

/** 订阅花费总览 fixture（issue #160）：面板挂载即拉取，默认空数据 */
const emptySpendOverview: SubscriptionSpendOverview = {
  native_currency: 'CNY',
  this_month_native_cents: 0,
  this_year_native_cents: 0,
  months: [],
  rows: [],
  projected_month_native_cents: 0,
  projected_year_native_cents: 0,
}
let mockSpendOverview: SubscriptionSpendOverview = emptySpendOverview

function baseInvoke() {
  mockInvoke.mockImplementation(((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve(mockCategories)
    if (cmd === 'list_merchants') return Promise.resolve(mockMerchantsState)
    if (cmd === 'subscription_spend_overview') return Promise.resolve(mockSpendOverview)
    if (cmd === 'list_scheduled_transactions') return Promise.resolve(mockPlans)
    if (cmd === 'get_scheduled_transaction_detail') {
      const detail = mockDetails.get(String(args?.id))
      return detail ? Promise.resolve(detail) : Promise.reject(new Error('无此计划详情'))
    }
    if (cmd === 'create_scheduled_transaction') {
      const input = args?.input as { kind: string; note: string | null; merchant_id: string | null }
      const id = `new-${input.kind}-${input.note ?? ''}`
      const plan = makePlan(
        { id, note: input.note ?? null },
        input.merchant_id,
      )
      mockPlans = [...mockPlans, plan]
      mockDetails.set(id, makeDetail(plan, []))
      return Promise.resolve(id)
    }
    if (cmd === 'create_merchant') {
      const input = args?.input as { name: string }
      const id = `mer-new-${input.name}`
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
    if (cmd === 'update_scheduled_subscription') {
      if (failSubscriptionUpdate) {
        return Promise.reject(new Error('订阅金额不可编辑：改价 = 取消旧计划 + 新建'))
      }
      const input = args?.input as {
        id: string
        account_id: string
        category_id: string | null
        merchant_id: string | null
        note: string | null
      }
      mockPlans = mockPlans.map((p) =>
        p.core.id === input.id
          ? {
              ...p,
              core: {
                ...p.core,
                account_id: input.account_id,
                category_id: input.category_id,
                note: input.note,
              },
              merchant_id: input.merchant_id,
            }
          : p,
      )
      const detail = mockDetails.get(input.id)
      if (detail) {
        mockDetails.set(input.id, {
          ...detail,
          core: {
            ...detail.core,
            account_id: input.account_id,
            category_id: input.category_id,
            note: input.note,
          },
          extension: { ...detail.extension, merchant_id: input.merchant_id },
        })
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
  failSubscriptionUpdate = false
  mockMerchantsState = mockMerchants
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

describe('SubscriptionsView 新建订阅模态对话框（issue #158）', () => {
  /** 点击「新建订阅」按钮打开模态对话框。 */
  async function openCreateModal(wrapper: ReturnType<typeof mount>) {
    await wrapper.find('[data-testid="sub-create-open"]').trigger('click')
    await flushPromises()
  }

  it('初始无弹窗，点击「新建订阅」打开模态对话框', async () => {
    const wrapper = await mountView()
    const modal = wrapper.findComponent(NModal)
    expect(modal.props('show')).toBe(false)
    await openCreateModal(wrapper)
    expect(modal.props('show')).toBe(true)
    expect(modal.props('title')).toBe('新建订阅')
  })

  it('创建成功后重置表单，重新打开为全新表单', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    const noteInput = findInput(wrapper, 'sub-note')
    await noteInput.setValue('音乐订阅')
    await noteInput.trigger('input')
    wrapper.findComponent(NSelect).vm.$emit('update:value', 'acc-1')
    await flushPromises()
    const amountInput = findInput(wrapper, 'sub-amount')
    await amountInput.setValue('25')
    await amountInput.trigger('input')
    await wrapper.findComponent('[data-testid="sub-create"]').trigger('click')
    await flushPromises()
    // 重新打开：备注已清空，不带上次填写
    await openCreateModal(wrapper)
    expect(findInput(wrapper, 'sub-note').element.value).toBe('')
  })

  it('仅关闭弹窗（不提交）不触发创建', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    wrapper.findComponent(NModal).vm.$emit('update:show', false)
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_scheduled_transaction'),
    ).toBe(false)
  })

  it('弹窗内填表创建走既有创建命令，金额转分、kind=subscription', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    const noteInput = findInput(wrapper, 'sub-note')
    await noteInput.setValue('音乐订阅')
    await noteInput.trigger('input')
    const amountInput = findInput(wrapper, 'sub-amount')
    await amountInput.setValue('25')
    await amountInput.trigger('input')
    // 账户 / 分类 / 周期：经组件 emit 设置
    wrapper.findComponent(NSelect).vm.$emit('update:value', 'acc-1')
    wrapper.findComponent(NTreeSelect).vm.$emit('update:value', 'cat-1')
    wrapper
      .findComponent(NDatePicker)
      .vm.$emit('update:formatted-value', '2026-02-15')
    await flushPromises()

    const createBtn = wrapper.findComponent('[data-testid="sub-create"]')
    await createBtn.trigger('click')
    await flushPromises()

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_scheduled_transaction')
    expect(call).toBeDefined()
    expect(call![1]).toEqual({
      input: {
        kind: 'subscription',
        account_id: 'acc-1',
        category_id: 'cat-1',
        merchant_id: null,
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
    await openCreateModal(wrapper)
    const amountInput = findInput(wrapper, 'sub-amount')
    await amountInput.setValue('25')
    await amountInput.trigger('input')
    await wrapper.findComponent('[data-testid="sub-create"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_scheduled_transaction'),
    ).toBe(false)
  })

  it('创建成功后关闭弹窗并刷新清单，新订阅出现在列表', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    const noteInput = findInput(wrapper, 'sub-note')
    await noteInput.setValue('云存储')
    await noteInput.trigger('input')
    const amountInput = findInput(wrapper, 'sub-amount')
    await amountInput.setValue('6')
    await amountInput.trigger('input')
    wrapper.findComponent(NSelect).vm.$emit('update:value', 'acc-1')
    await flushPromises()
    await wrapper.findComponent('[data-testid="sub-create"]').trigger('click')
    await flushPromises()
    // 弹窗关闭且清单刷新（新订阅出现在列表）
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    expect(wrapper.text()).toContain('云存储')
  })

  it('商户下拉补全在用商户：选中后创建携带 merchant_id（issue #190）', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    // 商户下拉 = 新建弹窗内 data-testid 为 sub-merchant 的 PinyinSelect（内部 NSelect 承载 options）
    const merchantSelect = wrapper
      .findComponent('[data-testid="sub-merchant"]')
      .findComponent(NSelect)
    expect(merchantSelect.exists()).toBe(true)
    const options = merchantSelect.props('options') as { label: string; value: string }[]
    expect(options.map((o) => o.label)).toEqual(['视频平台'])
    merchantSelect.vm.$emit('update:value', 'mer-1')
    const amountInput = findInput(wrapper, 'sub-amount')
    await amountInput.setValue('25')
    await amountInput.trigger('input')
    wrapper.findComponent(NSelect).vm.$emit('update:value', 'acc-1')
    await flushPromises()
    await wrapper.findComponent('[data-testid="sub-create"]').trigger('click')
    await flushPromises()
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_scheduled_transaction')
    expect(call![1]).toMatchObject({ input: { merchant_id: 'mer-1' } })
    // 列表商户列显示商户名
    expect(wrapper.text()).toContain('视频平台')
  })

  it('输入不存在的商户名保存即建：create_merchant 后按返回 id 创建计划（issue #190）', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    // 输入文本「新商户」：未命中在用商户 → 保存时即建
    wrapper.findComponent('[data-testid="sub-merchant"]').vm.$emit('update:value', '新商户')
    const amountInput = findInput(wrapper, 'sub-amount')
    await amountInput.setValue('25')
    await amountInput.trigger('input')
    wrapper.findComponent(NSelect).vm.$emit('update:value', 'acc-1')
    await flushPromises()
    await wrapper.findComponent('[data-testid="sub-create"]').trigger('click')
    await flushPromises()
    // 先即建商户，再携带返回的 id 创建计划
    const merchantCall = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_merchant')
    expect(merchantCall).toBeDefined()
    expect(merchantCall![1]).toEqual({ input: { name: '新商户' } })
    const createCall = mockInvoke.mock.calls.find(
      ([cmd]) => cmd === 'create_scheduled_transaction',
    )
    expect(createCall![1]).toMatchObject({ input: { merchant_id: 'mer-new-新商户' } })
  })
})

describe('SubscriptionsView 商户列（issue #190）', () => {
  it('列表显示计划商户（merchantMap 派生，改名即时生效）', async () => {
    const plan = makePlan({ id: 'a1', note: '视频会员' }, 'mer-1')
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('视频平台')
  })

  it('无商户计划显示 — 占位', async () => {
    const plan = makePlan({ id: 'a1', note: '视频会员' })
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('视频会员')
    // 商户列占位：不出现商户名
    expect(wrapper.text()).not.toContain('视频平台')
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

describe('SubscriptionsView 订阅编辑——仅非金额字段（issue #162）', () => {
  /** 按标题定位弹窗：页面有两个 NModal，findComponent 只返回第一个。 */
  function findModal(wrapper: ReturnType<typeof mount>, title: string) {
    const modal = wrapper
      .findAllComponents(NModal)
      .find((m) => m.props('title') === title)
    expect(modal, `应存在标题为「${title}」的弹窗`).toBeDefined()
    return modal!
  }

  /** 打开 a1 行的编辑弹窗。 */
  async function openEditModal(wrapper: ReturnType<typeof mount>) {
    await wrapper.find('[data-testid="op-edit-a1"]').trigger('click')
    await flushPromises()
  }

  it('进行中/已暂停行提供编辑入口，已取消行不提供', async () => {
    const plan = makePlan({ id: 'a1' })
    mockPlans = [plan, makePlan({ id: 'c1', status: 'cancelled', note: '已取消订阅' })]
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    expect(wrapper.find('[data-testid="op-edit-a1"]').exists()).toBe(true)
    // 已取消行不提供编辑（列表切到已取消后无编辑按钮）
    await wrapper.find('[data-testid="filter-cancelled"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="op-edit-c1"]').exists()).toBe(false)
  })

  it('编辑弹窗预填非金额字段且无金额输入', async () => {
    const plan = makePlan({
      id: 'a1',
      note: '视频会员',
      category_id: 'cat-1',
      account_id: 'acc-1',
      amount_cents: 1500,
    })
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    await openEditModal(wrapper)
    const modal = findModal(wrapper, '编辑订阅')
    expect(modal.props('show')).toBe(true)
    expect(modal.props('title')).toBe('编辑订阅')
    // 预填备注
    expect(findInput(wrapper, 'sub-edit-note').element.value).toBe('视频会员')
    // 无金额输入：无金额输入框、无数字步进（周期间隔）、无日期选择
    expect(wrapper.findComponent('[data-testid="sub-amount"]').exists()).toBe(false)
    expect(modal.findComponent(NInputNumber).exists()).toBe(false)
    expect(modal.findComponent(NDatePicker).exists()).toBe(false)
    // 弹窗内不出现计划金额
    expect(modal.text()).not.toContain('¥15')
  })

  it('未选账户时不提交编辑', async () => {
    const plan = makePlan({ id: 'a1' })
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    await openEditModal(wrapper)
    // 清空账户（编辑弹窗内唯一的 NSelect 是扣款账户）
    wrapper.findComponent(NSelect).vm.$emit('update:value', null)
    await flushPromises()
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'update_scheduled_subscription'),
    ).toBe(false)
  })

  it('提交编辑走订阅编辑命令，参数不含金额字段，成功后关闭弹窗并刷新清单', async () => {
    const plan = makePlan({ id: 'a1', note: '视频会员' }, 'mer-1')
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    await openEditModal(wrapper)
    const noteInput = findInput(wrapper, 'sub-edit-note')
    await noteInput.setValue('音乐会员')
    await noteInput.trigger('input')
    // 账户/分类经组件 emit（编辑弹窗打开时新建弹窗未渲染，实例唯一）
    wrapper.findComponent(NSelect).vm.$emit('update:value', 'acc-1')
    wrapper.findComponent(NTreeSelect).vm.$emit('update:value', 'cat-1')
    await flushPromises()
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger('click')
    await flushPromises()
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'update_scheduled_subscription')
    expect(call).toBeDefined()
    expect(call![1]).toEqual({
      input: {
        id: 'a1',
        account_id: 'acc-1',
        category_id: 'cat-1',
        merchant_id: 'mer-1',
        note: '音乐会员',
      },
    })
    // 弹窗关闭且清单刷新（新备注出现在列表）
    expect(findModal(wrapper, '编辑订阅').props('show')).toBe(false)
    expect(wrapper.text()).toContain('音乐会员')
  })

  it('编辑弹窗改商户：预填当前商户，保存携带新 merchant_id（issue #190）', async () => {
    // 第二个在用商户：编辑目标从 mer-1 改为 mer-2
    mockMerchantsState = [
      ...mockMerchants,
      {
        id: 'mer-2',
        name: '商户B',
        icon: null,
        color: null,
        created_at: '2026-01-01T00:00:00Z',
        updated_at: '2026-01-01T00:00:00Z',
        version: 1,
        device_id: 'test',
        is_deleted: false,
      },
    ]
    // beforeEach 已加载 store（新鲜度窗口内 ensureFresh 不会重拉），强制刷新拿新字典
    await useReferenceStore().refresh()
    const plan = makePlan({ id: 'a1', note: '视频会员' }, 'mer-1')
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    await openEditModal(wrapper)
    // 商户下拉 = 编辑弹窗内 data-testid 为 sub-edit-merchant 的 PinyinSelect（内部 NSelect）
    const merchantSelect = wrapper
      .findComponent('[data-testid="sub-edit-merchant"]')
      .findComponent(NSelect)
    expect(merchantSelect.exists()).toBe(true)
    expect(merchantSelect.props('value')).toBe('mer-1')
    merchantSelect.vm.$emit('update:value', 'mer-2')
    await flushPromises()
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger('click')
    await flushPromises()
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'update_scheduled_subscription')
    expect(call![1]).toMatchObject({ input: { merchant_id: 'mer-2' } })
    // 清单商户列刷新为新商户名
    expect(wrapper.text()).toContain('商户B')
  })

  it('提交失败时弹窗保持打开', async () => {
    const plan = makePlan({ id: 'a1' })
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    failSubscriptionUpdate = true
    await openEditModal(wrapper)
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger('click')
    await flushPromises()
    expect(findModal(wrapper, '编辑订阅').props('show')).toBe(true)
  })
})
