import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { NModal, NSelect, NInputNumber, NDatePicker, NPopconfirm } from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import TransfersPane from '@/components/scheduled/TransfersPane.vue'
import type {
  Account,
  Currency,
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
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  makeAccount('acc-cny1', '招商银行', 'CNY'),
  makeAccount('acc-cny2', '支付宝', 'CNY'),
  makeAccount('acc-usd', '美券商', 'USD'),
]

function makeAccount(id: string, name: string, currency_code: string): Account {
  return {
    id,
    name,
    type: 'cash',
    currency_code,
    initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    is_hidden: false,
  }
}

/** 定时转账计划工厂：core.kind 固定 scheduled_transfer，其余可覆写。 */
function makeTransferPlan(
  partial: Partial<ScheduledTransaction> & { id: string },
  to_account_id: string | null,
  total_occurrences: number | null = null,
): ScheduledTransactionWithExt {
  const core: ScheduledTransaction = {
    kind: 'scheduled_transfer',
    status: 'active',
    account_id: 'acc-cny1',
    category_id: null,
    amount_cents: 50000,
    currency_code: 'CNY',
    recurrence_type: 'monthly',
    recurrence_interval: 1,
    recurrence_day: null,
    start_date: '2026-01-01',
    note: null,
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
    total_amount_cents: null,
    total_occurrences,
    to_account_id,
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
    amount_cents: 50000,
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
      to_account_id: plan.to_account_id ?? '',
      total_occurrences: plan.total_occurrences,
    },
    pending_occurrences,
    completed_occurrences: 0,
  }
}

// —— invoke mock：可变数据源，状态操作后重载读得到最新值 ——
let mockPlans: ScheduledTransactionWithExt[] = []
const mockDetails = new Map<string, ScheduledTransactionDetail>()
/** 创建失败开关（后端拒绝币种不一致等场景） */
let failCreate = false

function baseInvoke() {
  mockInvoke.mockImplementation(((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
    if (cmd === 'list_scheduled_transactions') return Promise.resolve(mockPlans)
    if (cmd === 'get_scheduled_transaction_detail') {
      const detail = mockDetails.get(String(args?.id))
      return detail ? Promise.resolve(detail) : Promise.reject(new Error('无此计划详情'))
    }
    if (cmd === 'create_scheduled_transaction') {
      if (failCreate) {
        return Promise.reject(new Error('转出账户与转入账户币种不一致，定时转账不支持跨币种'))
      }
      const input = args?.input as {
        kind: string
        note: string | null
        to_account_id: string | null
        total_occurrences: number | null
      }
      const id = `new-transfer-${input.note ?? ''}`
      const plan = makeTransferPlan(
        { id, note: input.note ?? null },
        input.to_account_id,
        input.total_occurrences ?? null,
      )
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

/** 定位弹窗表单内输入框：NModal teleport 到 body，需经 findComponent 锚定。 */
function findInput(wrapper: ReturnType<typeof mount>, testid: string) {
  return wrapper.findComponent(`[data-testid="${testid}"]`).find('input')
}

async function mountView() {
  const wrapper = mount(TransfersPane)
  await flushPromises()
  return wrapper
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockPlans = []
  mockDetails.clear()
  failCreate = false
  baseInvoke()
  const store = useReferenceStore()
  await store.refresh()
})

describe('TransfersPane 定时转账清单（issue #203）', () => {
  it('只展示定时转账计划，订阅 / 分期不出现', async () => {
    const transfer = makeTransferPlan({ id: 't1', note: '月度储蓄' }, 'acc-cny2')
    mockPlans = [
      transfer,
      makeTransferPlan({ id: 's1', note: '某订阅', kind: 'subscription' }, null),
      makeTransferPlan({ id: 'i1', note: '某分期', kind: 'installment' }, null),
    ]
    mockDetails.set('t1', makeDetail(transfer, []))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('月度储蓄')
    expect(wrapper.text()).not.toContain('某订阅')
    expect(wrapper.text()).not.toContain('某分期')
  })

  it('清单展示转出→转入账户、金额、周期与下期转账日期', async () => {
    const transfer = makeTransferPlan(
      { id: 't1', note: '月度储蓄', amount_cents: 50000, recurrence_interval: 1 },
      'acc-cny2',
    )
    mockPlans = [transfer]
    mockDetails.set(
      't1',
      makeDetail(transfer, [
        makeOccurrence({ id: 'o2', scheduled_date: '2026-04-01' }),
        makeOccurrence({ id: 'o1', scheduled_date: '2026-03-01' }),
      ]),
    )
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('招商银行')
    expect(wrapper.text()).toContain('支付宝')
    expect(wrapper.text()).toContain('¥500')
    expect(wrapper.text()).toContain('每月')
    const cell = wrapper.find('[data-testid="next-transfer-t1"]')
    expect(cell.text()).toContain('2026-03-01')
    expect(cell.text()).not.toContain('2026-04-01')
  })

  it('状态过滤支持已完成（一次性转账执行后 completed）', async () => {
    const done = makeTransferPlan({ id: 'd1', note: '一次性转账', status: 'completed' }, 'acc-cny2')
    const active = makeTransferPlan({ id: 'a1', note: '循环转账' }, 'acc-cny2')
    mockPlans = [done, active]
    mockDetails.set('d1', makeDetail(done, []))
    mockDetails.set('a1', makeDetail(active, []))
    const wrapper = await mountView()
    expect(wrapper.text()).not.toContain('一次性转账')
    await wrapper.find('[data-testid="filter-completed"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('一次性转账')
    expect(wrapper.text()).not.toContain('循环转账')
  })

  it('已取消与已暂停行经对应过滤可见', async () => {
    const paused = makeTransferPlan({ id: 'p1', note: '已暂停转账', status: 'paused' }, 'acc-cny2')
    const cancelled = makeTransferPlan(
      { id: 'c1', note: '已取消转账', status: 'cancelled' },
      'acc-cny2',
    )
    mockPlans = [paused, cancelled]
    mockDetails.set('p1', makeDetail(paused, []))
    mockDetails.set('c1', makeDetail(cancelled, []))
    const wrapper = await mountView()
    await wrapper.find('[data-testid="filter-paused"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已暂停转账')
    expect(wrapper.text()).not.toContain('已取消转账')
    await wrapper.find('[data-testid="filter-cancelled"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已取消转账')
  })
})

describe('TransfersPane 新建定时转账（issue #203）', () => {
  /** 打开新建弹窗。 */
  async function openCreateModal(wrapper: ReturnType<typeof mount>) {
    await wrapper.find('[data-testid="transfer-create-open"]').trigger('click')
    await flushPromises()
  }

  /** 弹窗内转出 / 转入账户下拉（PinyinSelect 内部 NSelect 承载 options）。 */
  function accountSelect(wrapper: ReturnType<typeof mount>, testid: string) {
    return wrapper.findComponent(`[data-testid="${testid}"]`).findComponent(NSelect)
  }

  it('点击「新建转账」打开模态对话框', async () => {
    const wrapper = await mountView()
    const modal = wrapper.findComponent(NModal)
    expect(modal.props('show')).toBe(false)
    await openCreateModal(wrapper)
    expect(modal.props('show')).toBe(true)
  })

  it('转入账户候选按转出账户币种过滤（Vitest 验收项）', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    // 未选转出账户时，转入候选为全部账户
    const toSelect = accountSelect(wrapper, 'transfer-to-account')
    expect((toSelect.props('options') as { value: string }[]).map((o) => o.value)).toEqual([
      'acc-cny1',
      'acc-cny2',
      'acc-usd',
    ])
    // 选定 CNY 转出账户后，转入候选只剩其它 CNY 账户（排除转出账户本身）
    accountSelect(wrapper, 'transfer-from-account').vm.$emit('update:value', 'acc-cny1')
    await flushPromises()
    expect((toSelect.props('options') as { value: string }[]).map((o) => o.value)).toEqual([
      'acc-cny2',
    ])
    // 换成 USD 转出账户，候选只剩 USD 账户（同样排除转出账户本身）
    accountSelect(wrapper, 'transfer-from-account').vm.$emit('update:value', 'acc-usd')
    await flushPromises()
    expect((toSelect.props('options') as { value: string }[]).map((o) => o.value)).toEqual([])
  })

  it('切换转出账户后清空币种不再匹配的转入账户选中', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    accountSelect(wrapper, 'transfer-from-account').vm.$emit('update:value', 'acc-cny1')
    await flushPromises()
    accountSelect(wrapper, 'transfer-to-account').vm.$emit('update:value', 'acc-cny2')
    await flushPromises()
    // 换成 USD 转出账户：已选的 CNY 转入账户被清空
    accountSelect(wrapper, 'transfer-from-account').vm.$emit('update:value', 'acc-usd')
    await flushPromises()
    expect(accountSelect(wrapper, 'transfer-to-account').props('value')).toBeNull()
  })

  it('币种自动跟随转出账户币种', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    accountSelect(wrapper, 'transfer-from-account').vm.$emit('update:value', 'acc-usd')
    await flushPromises()
    // 币种下拉现经 AppSelect 封装（接入弹层注册表），下探到内层 NSelect 取 props
    const currencySelect = wrapper
      .findComponent('[data-testid="transfer-currency"]')
      .findComponent(NSelect)
    expect(currencySelect.props('value')).toBe('USD')
  })

  it('创建无限循环转账：不填总期数提交 total_occurrences=null', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    accountSelect(wrapper, 'transfer-from-account').vm.$emit('update:value', 'acc-cny1')
    await flushPromises()
    accountSelect(wrapper, 'transfer-to-account').vm.$emit('update:value', 'acc-cny2')
    await flushPromises()
    const amountInput = findInput(wrapper, 'transfer-amount')
    await amountInput.setValue('500')
    await amountInput.trigger('input')
    await wrapper.findComponent('[data-testid="transfer-create"]').trigger('click')
    await flushPromises()

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_scheduled_transaction')
    expect(call).toBeDefined()
    expect(call![1]).toMatchObject({
      input: {
        kind: 'scheduled_transfer',
        account_id: 'acc-cny1',
        to_account_id: 'acc-cny2',
        amount_cents: 50000,
        currency_code: 'CNY',
        total_occurrences: null,
        merchant_id: null,
        category_id: null,
      },
    })
  })

  it('创建一次性（总期数=1）与有限期数（总期数=3）转账', async () => {
    for (const n of [1, 3]) {
      mockPlans = []
      mockInvoke.mockClear()
      const wrapper = await mountView()
      await openCreateModal(wrapper)
      accountSelect(wrapper, 'transfer-from-account').vm.$emit('update:value', 'acc-cny1')
      await flushPromises()
      accountSelect(wrapper, 'transfer-to-account').vm.$emit('update:value', 'acc-cny2')
      await flushPromises()
      const amountInput = findInput(wrapper, 'transfer-amount')
      await amountInput.setValue('100')
      await amountInput.trigger('input')
      wrapper
        .findComponent('[data-testid="transfer-total-occurrences"]')
        .vm.$emit('update:value', n)
      await flushPromises()
      await wrapper.findComponent('[data-testid="transfer-create"]').trigger('click')
      await flushPromises()
      const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_scheduled_transaction')
      expect(call![1]).toMatchObject({ input: { total_occurrences: n } })
    }
  })

  it('创建成功后关闭弹窗并刷新清单，新转账出现在列表', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    accountSelect(wrapper, 'transfer-from-account').vm.$emit('update:value', 'acc-cny1')
    await flushPromises()
    accountSelect(wrapper, 'transfer-to-account').vm.$emit('update:value', 'acc-cny2')
    await flushPromises()
    const amountInput = findInput(wrapper, 'transfer-amount')
    await amountInput.setValue('500')
    await amountInput.trigger('input')
    await wrapper.findComponent('[data-testid="transfer-create"]').trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    expect(wrapper.text()).toContain('支付宝')
  })

  it('未选转出 / 转入账户或金额为 0 时不提交创建', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    // 什么都没填
    await wrapper.findComponent('[data-testid="transfer-create"]').trigger('click')
    await flushPromises()
    // 只选账户不填金额
    accountSelect(wrapper, 'transfer-from-account').vm.$emit('update:value', 'acc-cny1')
    accountSelect(wrapper, 'transfer-to-account').vm.$emit('update:value', 'acc-cny2')
    await flushPromises()
    await wrapper.findComponent('[data-testid="transfer-create"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_scheduled_transaction'),
    ).toBe(false)
  })

  it('后端拒绝（币种不一致）时错误提示且弹窗保持打开', async () => {
    failCreate = true
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    accountSelect(wrapper, 'transfer-from-account').vm.$emit('update:value', 'acc-cny1')
    accountSelect(wrapper, 'transfer-to-account').vm.$emit('update:value', 'acc-cny2')
    await flushPromises()
    const amountInput = findInput(wrapper, 'transfer-amount')
    await amountInput.setValue('500')
    await amountInput.trigger('input')
    await wrapper.findComponent('[data-testid="transfer-create"]').trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(true)
    // 错误提示经 useMessage 呈现（setup.ts mock 为 spy，不渲染 DOM），
    // 这里断言弹窗保持打开即可体现拒绝路径
  })

  it('弹窗不出现商户字段（定时转账不使用商户）', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    expect(wrapper.findComponent('[data-testid="transfer-merchant"]').exists()).toBe(false)
  })
})

describe('TransfersPane 状态操作（issue #203）', () => {
  it('进行中的转账可暂停（走既有状态命令）', async () => {
    const plan = makeTransferPlan({ id: 'a1' }, 'acc-cny2')
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
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

  it('已暂停的转账可恢复', async () => {
    const plan = makeTransferPlan({ id: 'p1', status: 'paused' }, 'acc-cny2')
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
          (args as { input: { new_status: string } }).input.new_status === 'active',
      ),
    ).toBe(true)
  })

  it('取消需二次确认（NPopconfirm），确认后走状态命令', async () => {
    const plan = makeTransferPlan({ id: 'a1' }, 'acc-cny2')
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
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
          (args as { input: { new_status: string } }).input.new_status === 'cancelled',
      ),
    ).toBe(true)
  })

  it('已完成 / 已取消的转账不再提供状态操作', async () => {
    const done = makeTransferPlan({ id: 'd1', status: 'completed' }, 'acc-cny2')
    mockPlans = [done]
    mockDetails.set('d1', makeDetail(done, []))
    const wrapper = await mountView()
    await wrapper.find('[data-testid="filter-completed"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="op-pause-d1"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="op-resume-d1"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="op-cancel-d1"]').exists()).toBe(false)
  })
})
