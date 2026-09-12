import { describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from '../helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { NDataTable, NModal, NSelect, NPopconfirm } from 'naive-ui'
import TransfersPane from '@/components/scheduled/TransfersPane.vue'
import { findInputByTestId as findInput } from '../helpers/dom'
import { mountFlushed } from '../helpers/mount'
import { setFakeMedia } from '../helpers/media-mock'
import { makeOccurrence, makeTransferPlan } from '../factories'
import { formatAmount } from '@/utils/money'
import type {
  Account,
  Currency,
  ScheduledStatus,
  ScheduledTransactionDetail,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
} from '@ledger/types'
import { componentVm } from '../helpers/component-vm'

/**
 * 定时转账页签组件测试（ADR-0041 决策 10）：清单加载/状态过滤/生命周期状态机等
 * 时序用例已迁 ScheduledPlanList 模块接口测试（useScheduledPlanList.test.ts，
 * 刷新版本号镜像法）；本文件收缩为渲染与交互冒烟 + 转账形态真差异（表单）。
 * 迁移记录见对应提交信息。
 */

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
]

// 金额断言委托形态（issue #770）：期待值调同一 formatAmount 实现，格式规则唯一归属其专测
const cny = mockCurrencies[0]

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

/** 转账详情组装（目录特有派生包装，留守本地）。 */
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
    completed_amount_cents: 0,
    occurrences: pending_occurrences,
  }
}

// —— invoke mock：可变数据源，状态操作后重载读得到最新值 ——
let mockPlans: ScheduledTransactionWithExt[] = []
const mockDetails = new Map<string, ScheduledTransactionDetail>()
/** 创建失败开关（后端拒绝币种不一致等场景） */
let failCreate = false

function mountView() {
  return mountFlushed(TransfersPane)
}

beforeEach(async () => {
  mockPlans = []
  mockDetails.clear()
  failCreate = false
  // 唯一接缝布线（ADR-0085）：币种（含 USD）与账户以本套夹具覆写（值与规范
  // 夹具不同，属场景契约而非重复枚举），进 defaults 表；可变计划库与创建/
  // 状态编排为函数型 overrides。其余参考命令由规范夹具兑底；store 层预热
  // opt-in 开启：主题用例依赖参考数据就绪后的即时渲染（账户名、币种符号）。
  const seam = wireInvokeSeam({
    defaults: {
      list_currencies: mockCurrencies,
      list_accounts: mockAccounts,
    },
    overrides: {
      list_scheduled_transactions: () => mockPlans,
      get_scheduled_transaction_detail: (args) => {
        const detail = mockDetails.get(String(args?.id))
        return detail ? Promise.resolve(detail) : Promise.reject(new Error('无此计划详情'))
      },
      create_scheduled_transaction: (args) => {
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
        return id
      },
      update_scheduled_transaction_status: (args) => {
        const { id, new_status } = args as { id: string; new_status: ScheduledStatus }
        mockPlans = mockPlans.map((p) =>
          p.core.id === id ? { ...p, core: { ...p.core, status: new_status } } : p,
        )
        const detail = mockDetails.get(id)
        if (detail) {
          mockDetails.set(id, { ...detail, core: { ...detail.core, status: new_status } })
        }
      },
    },
    refreshReferenceStores: true,
  })
  await seam.ready
})

describe('TransfersPane 清单渲染冒烟（编排用例见 useScheduledPlanList.test.ts）', () => {
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
      { id: 't1', note: '月度储蓄', amount_cents: 50000, recurrence_interval: 2 },
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
    expect(wrapper.text()).toContain(formatAmount(50000, cny))
    expect(wrapper.text()).toContain('每2月')
    const cell = wrapper.find('[data-testid="next-transfer-t1"]')
    expect(cell.text()).toContain('2026-03-01')
    expect(cell.text()).not.toContain('2026-04-01')
  })

  it('状态过滤按钮接线：completed 行经「已完成」过滤可见', async () => {
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
})

describe('TransfersPane 操作列渲染与确认交互（可用性矩阵与状态机见模块测试）', () => {
  it('active 行点「暂停」发出状态命令（交互冒烟：描述符 → 按钮 onClick 接线）', async () => {
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

  it('取消需二次确认（AppPopconfirm），确认后走状态命令', async () => {
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
})

describe('TransfersPane 新建定时转账（转账形态真差异，issue #203）', () => {
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

  it('新建表单周期下拉统一为「每天/每周/每月/每年」（#309 显式可见变化，单源选项表）', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    const recurrenceSelect = wrapper
      .findComponent('[data-testid="transfer-recurrence"]')
      .findComponent(NSelect)
    expect(recurrenceSelect.props('options')).toEqual([
      { label: '每天', value: 'daily' },
      { label: '每周', value: 'weekly' },
      { label: '每月', value: 'monthly' },
      { label: '每年', value: 'yearly' },
    ])
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

  // 提交流程编排（商户解析跳过 → payload 合并 → 创建 → 提示 → 重置 → 回调）已迁移至接缝接口测试
  // （useScheduledPlanForm.test.ts「submitCreate 提交时序编排」）。此处保留：交互冒烟（关窗 + 清单刷新接线）
  // 与本页签职责的形态特化字段组装（yuanToCents 元转分 + to_account_id/total_occurrences 直传）。

  it('创建成功后关闭弹窗并刷新清单，新转账出现在列表（页签直传特化字段与元转分）', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    accountSelect(wrapper, 'transfer-from-account').vm.$emit('update:value', 'acc-cny1')
    await flushPromises()
    accountSelect(wrapper, 'transfer-to-account').vm.$emit('update:value', 'acc-cny2')
    await flushPromises()
    const amountInput = findInput(wrapper, 'transfer-amount')
    await amountInput.setValue('500')
    await amountInput.trigger('input')
    // 特化字段组装（页签职责）：总期数 N
    componentVm(wrapper.findComponent('[data-testid="transfer-total-occurrences"]')).$emit('update:value', 3)
    await flushPromises()
    await wrapper.findComponent('[data-testid="transfer-create"]').trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    expect(wrapper.text()).toContain('支付宝')
    // 元转分（yuanToCents）与特化字段直传（非空断言留给接缝直测）
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_scheduled_transaction')
    expect(call).toBeDefined()
    expect(call![1]).toMatchObject({
      input: {
        kind: 'scheduled_transfer',
        account_id: 'acc-cny1',
        to_account_id: 'acc-cny2',
        amount_cents: 50000,
        total_occurrences: 3,
        merchant_id: null,
      },
    })
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

describe('TransfersPane 移动档（issue #848 / ADR-0088 决策 11 票⑧）', () => {
  /** 转账夹具：循环转账 招商银行 → 支付宝，每月，下期 2026-03-01。 */
  function wirePlan() {
    const plan = makeTransferPlan(
      { id: 't1', note: '月度储蓄', amount_cents: 50000 },
      'acc-cny2',
      null,
    )
    mockPlans = [plan]
    mockDetails.set(
      't1',
      makeDetail(plan, [makeOccurrence({ id: 'o1', scheduled_date: '2026-03-01' })]),
    )
    return plan
  }

  function tableOf(wrapper: Awaited<ReturnType<typeof mountView>>) {
    return wrapper.findComponent(NDataTable)
  }

  it('移动档列结构三分（备注/金额/操作），桌面档九列一字不动', async () => {
    wirePlan()
    const desktop = await mountView()
    expect((tableOf(desktop).props('columns') as unknown[]).length).toBe(9)
    desktop.unmount()

    setFakeMedia({ width: 600 })
    const mobile = await mountView()
    const columns = tableOf(mobile).props('columns') as Array<{ key?: string }>
    expect(columns.map((c) => c.key)).toEqual(['note', 'amount', 'actions'])
  })

  it('移动档信息并入副行不丢失：状态/周期/开始日/双向账户/金额/下期转账', async () => {
    setFakeMedia({ width: 600 })
    wirePlan()
    const wrapper = await mountView()
    const row = wrapper.find('.n-data-table-tbody .n-data-table-tr')
    const text = row.text()
    expect(text).toContain('月度储蓄')
    expect(text).toContain('进行中')
    expect(text).toContain('每月')
    expect(text).toContain('2026-01-01')
    // 双向账户：转出与转入并存（转账形态真差异）
    expect(text).toContain('招商银行')
    expect(text).toContain('支付宝')
    expect(text).toContain(formatAmount(50000, cny))
    // 下期转账锚点与桌面同 testid
    const next = wrapper.find('[data-testid="next-transfer-t1"]')
    expect(next.exists()).toBe(true)
    expect(next.text()).toContain('2026-03-01')
  })

  it('移动档生命周期操作一击可达：暂停/取消为可见按钮且 ≥48px 触控目标，取消经确认弹层', async () => {
    setFakeMedia({ width: 600 })
    wirePlan()
    const wrapper = await mountView()
    for (const key of ['pause', 'cancel']) {
      const btn = wrapper.find(`[data-testid="op-${key}-t1"]`)
      expect(btn.exists(), `应存在可见的「${key}」按钮`).toBe(true)
      const el = btn.element as HTMLElement
      expect(el.style.minWidth).toBe('48px')
      expect(el.style.minHeight).toBe('48px')
    }
    // 取消仍需二次确认：确认后走同一状态命令
    await wrapper.find('[data-testid="op-cancel-t1"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'update_scheduled_transaction_status'),
    ).toBe(false)
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

  it('跨断点缩窗实时换列（九列 ⇄ 三列）', async () => {
    wirePlan()
    const wrapper = await mountView()
    expect((tableOf(wrapper).props('columns') as unknown[]).length).toBe(9)
    setFakeMedia({ width: 600 })
    await flushPromises()
    expect((tableOf(wrapper).props('columns') as unknown[]).length).toBe(3)
    setFakeMedia({ width: 1280 })
    await flushPromises()
    expect((tableOf(wrapper).props('columns') as unknown[]).length).toBe(9)
  })
})
