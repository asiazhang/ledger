import { describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import {
  NDataTable,
  NModal,
  NSelect,
  NPopconfirm,
  NProgress,
} from 'naive-ui'
import InstallmentsPane from '@/components/scheduled/InstallmentsPane.vue'
import { findInputByTestId as findInput } from '@ledger/test-support/dom'
import { mountFlushed } from '@ledger/test-support/mount'
import { setFakeMedia } from '@ledger/test-support/media-mock'
import {
  makeInstallmentPlan,
  makeSubscriptionPlan,
  makeTransferPlan,
} from '../factories'
import { componentVm } from '@ledger/test-support/component-vm'
import { refCurrencies } from '@ledger/test-support/reference-stubs'
import { formatAmount } from '@/utils/money'
import type {
  Account,
  Category,
  InstallmentPlan,
  Merchant,
  ScheduledStatus,
  ScheduledTransactionDetail,
  ScheduledTransactionWithExt,
} from '@ledger/types'

// 金额断言委托形态（issue #770）：期待值调同一 formatAmount 实现，格式规则唯一归属其专测
const cny = refCurrencies[0]

/**
 * 分期页签组件测试（ADR-0041 决策 10，迁移步 3）：清单加载/按形态过滤/状态过滤/
 * Plan Lifecycle 状态机（参数/提示/重拉时序/可用性矩阵）已由 ScheduledPlanList
 * 模块接口测试承接（useScheduledPlanList.test.ts，刷新版本号镜像法）；商户解析
 * 竞态矩阵已由计划表单接缝测试承接（useScheduledPlanForm.test.ts）。本文件收缩为
 * 渲染与交互冒烟 + 分期形态真差异——期数预览（含尾差文案）、进度列（expandDetail
 * 接线：期数/金额取自详情命令）与新建表单校验/提交编排。迁移与删除记录见对应提交信息。
 */

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
    id: 'mer-1',
    name: '京东白条',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

/** 可变商户字典：状态操作 / 即建后重载读得到最新值。 */
let mockMerchantsState: Merchant[] = mockMerchants

/** 分期详情组装（目录特有派生包装，留守本地）。 */
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
    occurrences: [],
  }
}

// —— invoke mock：可变数据源，状态操作后重载读得到最新值 ——
let mockPlans: ScheduledTransactionWithExt[] = []
const mockDetails = new Map<string, ScheduledTransactionDetail>()


/** 弹窗内普通元素（非组件）经 document.body 查询：NModal teleport 到 body。 */
function modalText(testid: string) {
  return document.body.querySelector(`[data-testid="${testid}"]`)?.textContent ?? ''
}

function mountView() {
  return mountFlushed(InstallmentsPane)
}

beforeEach(async () => {
  mockPlans = []
  mockDetails.clear()
  mockMerchantsState = mockMerchants
  // 唯一接缝布线（ADR-0085）：账户与分类以本套夹具覆写（值与规范夹具不同，
  // 属场景契约而非重复枚举），进 defaults 表；可变商户库与创建/详情/状态
  // 编排为函数型 overrides。其余参考命令由规范夹具兑底；store 层预热
  // opt-in 开启：主题用例依赖参考数据就绪后的即时渲染（商户列、下拉选项）。
  const seam = wireInvokeSeam({
    defaults: {
      list_accounts: mockAccounts,
      list_categories: mockCategories,
    },
    overrides: {
      list_merchants: () => mockMerchantsState,
      create_merchant: (args) => {
        const input = args?.input as { name: string }
        const id = `mer-new-${input.name}`
        mockMerchantsState = [
          ...mockMerchantsState,
          {
            id,
            name: input.name,
            updated_at: '2026-01-01T00:00:00Z',
            version: 1,
            device_id: 'test',
            is_deleted: false,
          },
        ]
        return id
      },
      list_scheduled_transactions: () => mockPlans,
      get_scheduled_transaction_detail: (args) => {
        const detail = mockDetails.get(String(args?.id))
        return detail ? Promise.resolve(detail) : Promise.reject(new Error('无此计划详情'))
      },
      create_scheduled_transaction: (args) => {
        const input = args?.input as {
          kind: string
          note: string | null
          merchant_id: string | null
        }
        const id = `new-${input.kind}-${input.note ?? ''}`
        const createInput = args!.input as { total_amount_cents: number; total_occurrences: number }
        const plan = makeInstallmentPlan(
          { id, note: input.note ?? null },
          createInput.total_amount_cents ?? 0,
          createInput.total_occurrences ?? 1,
          input.merchant_id,
        )
        mockPlans = [...mockPlans, plan]
        mockDetails.set(id, makeDetail(plan, { count: 0, amount: 0 }))
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

describe('InstallmentsPane 清单渲染冒烟（编排用例见 useScheduledPlanList.test.ts）', () => {
  it('只展示分期计划，订阅 / 定时转账不出现（按形态过滤归模块，此处验渲染）', async () => {
    const inst = makeInstallmentPlan({ id: 'i1', note: '手机分期' }, 120000, 12)
    mockPlans = [
      inst,
      makeSubscriptionPlan({ id: 's1', note: '某订阅' }),
      makeTransferPlan({ id: 't1', note: '某定时转账' }, null),
    ]
    mockDetails.set('i1', makeDetail(inst, { count: 3, amount: 30000 }))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('手机分期')
    expect(wrapper.text()).not.toContain('某订阅')
    expect(wrapper.text()).not.toContain('某定时转账')
  })

  it('进度格显示进度条 + 已还金额/总额 · X/N 期（expandDetail 接线：期数与金额取自详情命令）', async () => {
    const inst = makeInstallmentPlan({ id: 'i1', note: '手机分期' }, 120000, 12)
    mockPlans = [inst]
    mockDetails.set('i1', makeDetail(inst, { count: 3, amount: 30000 }))
    const wrapper = await mountView()
    const cell = wrapper.find('[data-testid="inst-progress-i1"]')
    expect(cell.text()).toContain(formatAmount(30000, cny))
    expect(cell.text()).toContain(formatAmount(120000, cny))
    expect(cell.text()).toContain('3/12 期')
    const progress = cell.findComponent(NProgress)
    expect(progress.exists()).toBe(true)
    expect(progress.props('percentage')).toBe(25)
  })

  it('已完成金额来自详情命令的 completed_amount_cents，不由前端推算', async () => {
    // 1200 分 12 期每期应为 100，但已完成汇总给 150（模拟失败重试等真实历史）：
    // 显示以汇总为准，不用 total/occurrences 推算
    const inst = makeInstallmentPlan({ id: 'i1', note: '手机分期' }, 1200, 12)
    mockPlans = [inst]
    mockDetails.set('i1', makeDetail(inst, { count: 1, amount: 150 }))
    const wrapper = await mountView()
    expect(wrapper.find('[data-testid="inst-progress-i1"]').text()).toContain(formatAmount(150, cny))
  })

  it('默认只显示进行中（active）的分期，可切换过滤（默认过滤归模块，此处验渲染）', async () => {
    const a1 = makeInstallmentPlan({ id: 'a1', note: '进行中分期' }, 1200, 12)
    const p1 = makeInstallmentPlan({ id: 'p1', note: '已暂停分期', status: 'paused' }, 600, 6)
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

  it('状态过滤含「已完成」：completed 行经「已完成」过滤可见（#309 显式可见变化之二，迁移步 3 落地）', async () => {
    const done = makeInstallmentPlan({ id: 'd1', note: '已还清分期', status: 'completed' }, 1200, 12)
    const active = makeInstallmentPlan({ id: 'a1', note: '进行中分期' }, 1200, 12)
    mockPlans = [done, active]
    mockDetails.set('d1', makeDetail(done, { count: 12, amount: 1200 }))
    mockDetails.set('a1', makeDetail(active, { count: 0, amount: 0 }))
    const wrapper = await mountView()
    // 默认「进行中」：已完成分期不出现
    expect(wrapper.text()).not.toContain('已还清分期')
    await wrapper.find('[data-testid="filter-completed"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已还清分期')
    expect(wrapper.text()).not.toContain('进行中分期')
    // 已完成分期仅提供期次查看（可用性矩阵归模块，此处验渲染接线）
    expect(wrapper.find('[data-testid="op-detail-d1"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="op-pause-d1"]').exists()).toBe(false)
  })
})

describe('InstallmentsPane 操作列渲染与确认交互（可用性矩阵与状态机见模块测试）', () => {
  it('active 行点「暂停」发出状态命令（交互冒烟：描述符 → 按钮 onClick 接线）', async () => {
    const plan = makeInstallmentPlan({ id: 'a1' }, 1200, 12)
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
    const plan = makeInstallmentPlan({ id: 'p1', status: 'paused' }, 1200, 12)
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
    const plan = makeInstallmentPlan({ id: 'a1' }, 1200, 12)
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

  it('已取消的分期不再提供状态操作（可用性矩阵归模块，此处验渲染接线）', async () => {
    const plan = makeInstallmentPlan({ id: 'c1', status: 'cancelled', note: '已取消分期' }, 1200, 12)
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

describe('InstallmentsPane 新建分期（分期形态真差异，issue #204）', () => {
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

  it('弹窗不出现「每月几号」字段（#204 边界，商户字段由 #206 引入）', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    const modal = wrapper.findComponent(NModal)
    expect(modal.text()).not.toContain('几号')
  })

  it('填总额与期数实时预览每期金额与末期（含尾差）', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    // 未填时无预览
    expect(modalText('inst-preview')).toBe('')
    await findInput(wrapper, 'inst-total').setValue('1')
    await findInput(wrapper, 'inst-total').trigger('input')
    componentVm(wrapper.findComponent('[data-testid="inst-periods"]')).$emit('update:value', 3)
    await flushPromises()
    const preview = modalText('inst-preview')
    // 1 元分 3 期：每期 floor 33 分，末期 34 分（尾差归末期）
    expect(preview).toContain(formatAmount(33, cny))
    expect(preview).toContain(formatAmount(34, cny))
    expect(preview).toContain('尾差')
  })

  it('整除时末期与每期相等，预览不提尾差', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    await findInput(wrapper, 'inst-total').setValue('1200')
    await findInput(wrapper, 'inst-total').trigger('input')
    componentVm(wrapper.findComponent('[data-testid="inst-periods"]')).$emit('update:value', 12)
    await flushPromises()
    const preview = modalText('inst-preview')
    expect(preview).toContain(formatAmount(10000, cny))
    expect(preview).not.toContain('尾差')
  })

  // 提交流程编排（商户解析 → payload 合并 → 创建 → 提示 → 重置 → 回调）已迁移至接缝接口测试
  // （useScheduledPlanForm.test.ts「submitCreate 提交时序编排」分期形态用例）。此处保留：
  // 交互冒烟（关窗 + 清单刷新接线）、校验（留页签）与每期 floor 口径/特化字段直传接线。

  it('创建成功后关闭弹窗并刷新清单，新分期出现在列表（页签直传 floor 口径与特化字段）', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    await findInput(wrapper, 'inst-note').setValue('手机分期')
    await findInput(wrapper, 'inst-note').trigger('input')
    // 总额 1000 元 = 100000 分，分 12 期：floor(100000/12)=8333（floor 口径页签持有）
    await findInput(wrapper, 'inst-total').setValue('1000')
    await findInput(wrapper, 'inst-total').trigger('input')
    componentVm(wrapper.findComponent('[data-testid="inst-periods"]')).$emit('update:value', 12)
    componentVm(wrapper.findComponent('[data-testid="inst-account"]')).$emit('update:value', 'acc-1')
    await flushPromises()
    await wrapper.findComponent('[data-testid="inst-create"]').trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    expect(wrapper.text()).toContain('手机分期')
    // 元转分 + floor 口径 + 特化字段直传（公共字段与商户解析断言留给接缝直测）
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_scheduled_transaction')
    expect(call).toBeDefined()
    expect(call![1]).toMatchObject({
      input: {
        kind: 'installment',
        account_id: 'acc-1',
        amount_cents: 8333,
        total_amount_cents: 100000,
        total_occurrences: 12,
      },
    })
  })

  it('未填总额或期数时不提交', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    componentVm(wrapper.findComponent('[data-testid="inst-account"]')).$emit('update:value', 'acc-1')
    await flushPromises()
    // 只填期数不填总额
    componentVm(wrapper.findComponent('[data-testid="inst-periods"]')).$emit('update:value', 12)
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
    componentVm(wrapper.findComponent('[data-testid="inst-periods"]')).$emit('update:value', 3)
    componentVm(wrapper.findComponent('[data-testid="inst-account"]')).$emit('update:value', 'acc-1')
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
    componentVm(wrapper.findComponent('[data-testid="inst-periods"]')).$emit('update:value', 12)
    componentVm(wrapper.findComponent('[data-testid="inst-account"]')).$emit('update:value', 'acc-1')
    await flushPromises()
    await wrapper.findComponent('[data-testid="inst-create"]').trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    expect(wrapper.text()).toContain('手机分期')
  })
})

describe('InstallmentsPane 商户挂靠（issue #206：表单接缝接线冒烟，解析矩阵见 useScheduledPlanForm.test.ts）', () => {
  /** 点击「新建分期」打开模态对话框。 */
  async function openCreateModal(wrapper: ReturnType<typeof mount>) {
    await wrapper.find('[data-testid="inst-create-open"]').trigger('click')
    await flushPromises()
  }

  /** 填写除商户外的必填项（总额 1200 元分 12 期 + 扣款账户）。 */
  async function fillRequired(wrapper: ReturnType<typeof mount>) {
    await findInput(wrapper, 'inst-total').setValue('1200')
    await findInput(wrapper, 'inst-total').trigger('input')
    componentVm(wrapper.findComponent('[data-testid="inst-periods"]')).$emit('update:value', 12)
    componentVm(wrapper.findComponent('[data-testid="inst-account"]')).$emit('update:value', 'acc-1')
    await flushPromises()
  }

  it('商户下拉补全在用商户：选中后创建携带 merchant_id', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    // 商户下拉 = 新建弹窗内 data-testid 为 inst-merchant 的 PinyinSelect（内部 NSelect 承载 options）
    const merchantSelect = wrapper
      .findComponent('[data-testid="inst-merchant"]')
      .findComponent(NSelect)
    expect(merchantSelect.exists()).toBe(true)
    const options = merchantSelect.props('options') as { label: string; value: string }[]
    expect(options.map((o) => o.label)).toEqual(['京东白条'])
    merchantSelect.vm.$emit('update:value', 'mer-1')
    await fillRequired(wrapper)
    await wrapper.findComponent('[data-testid="inst-create"]').trigger('click')
    await flushPromises()
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_scheduled_transaction')
    expect(call![1]).toMatchObject({ input: { merchant_id: 'mer-1' } })
    // 清单商户列显示商户名
    expect(wrapper.text()).toContain('京东白条')
  })

  it('输入不存在的商户名保存即建：解析全仓单点走表单接缝，此处仅验接线（选中/即建矩阵见 useScheduledPlanForm.test.ts）', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    // 输入文本「新商户」：未命中在用商户 → 保存时接缝即建
    componentVm(wrapper.findComponent('[data-testid="inst-merchant"]')).$emit('update:value', '新商户')
    await fillRequired(wrapper)
    await wrapper.findComponent('[data-testid="inst-create"]').trigger('click')
    await flushPromises()
    // 提交携带解析后的商户 id（即建/重名兜底矩阵在接缝测试）
    const createCall = mockInvoke.mock.calls.find(
      ([cmd]) => cmd === 'create_scheduled_transaction',
    )
    expect(createCall![1]).toMatchObject({ input: { merchant_id: 'mer-new-新商户' } })
  })

  it('未选商户创建携带 null，不调用 create_merchant', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    await fillRequired(wrapper)
    await wrapper.findComponent('[data-testid="inst-create"]').trigger('click')
    await flushPromises()
    expect(mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_merchant')).toBeUndefined()
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_scheduled_transaction')
    expect(call![1]).toMatchObject({ input: { merchant_id: null } })
  })

  it('清单显示计划商户（merchantMap 派生，改名即时生效）', async () => {
    const inst = makeInstallmentPlan({ id: 'i1', note: '手机分期' }, 120000, 12, 'mer-1')
    mockPlans = [inst]
    mockDetails.set('i1', makeDetail(inst, { count: 0, amount: 0 }))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('手机分期')
    expect(wrapper.text()).toContain('京东白条')
  })

  it('无商户计划不显示商户名', async () => {
    const inst = makeInstallmentPlan({ id: 'i1', note: '手机分期' }, 120000, 12)
    mockPlans = [inst]
    mockDetails.set('i1', makeDetail(inst, { count: 0, amount: 0 }))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('手机分期')
    expect(wrapper.text()).not.toContain('京东白条')
  })
})

describe('InstallmentsPane 移动档（issue #848 / ADR-0088 决策 11 票⑧）', () => {
  /** 分期夹具：120000 分 12 期，已还 3 期 30000 分。 */
  function wirePlan() {
    const inst = makeInstallmentPlan(
      { id: 'i1', note: '手机分期' },
      120000,
      12,
      'mer-1',
    )
    mockPlans = [inst]
    mockDetails.set('i1', makeDetail(inst, { count: 3, amount: 30000 }))
    return inst
  }

  function tableOf(wrapper: Awaited<ReturnType<typeof mountView>>) {
    return wrapper.findComponent(NDataTable)
  }

  it('移动档列结构三分（备注/总额/操作），桌面档十列一字不动', async () => {
    wirePlan()
    const desktop = await mountView()
    expect((tableOf(desktop).props('columns') as unknown[]).length).toBe(10)
    desktop.unmount()

    setFakeMedia({ width: 600 })
    const mobile = await mountView()
    const columns = tableOf(mobile).props('columns') as Array<{ key?: string }>
    expect(columns.map((c) => c.key)).toEqual(['note', 'total', 'actions'])
  })

  it('移动档信息并入副行不丢失：状态/周期/开始日/商户/分类/账户/总额/进度（锚点同桌面）', async () => {
    setFakeMedia({ width: 600 })
    wirePlan()
    const wrapper = await mountView()
    const row = wrapper.find('.n-data-table-tbody .n-data-table-tr')
    const text = row.text()
    expect(text).toContain('手机分期')
    expect(text).toContain('进行中')
    expect(text).toContain('每月')
    expect(text).toContain('2026-01-01')
    expect(text).toContain('京东白条')
    expect(text).toContain('数码分期')
    expect(text).toContain('招商银行')
    expect(text).toContain(formatAmount(120000, cny))
    // 进度锚点与桌面同 testid：进度条 + 已还文案照常
    const cell = wrapper.find('[data-testid="inst-progress-i1"]')
    expect(cell.exists()).toBe(true)
    expect(cell.text()).toContain(formatAmount(30000, cny))
    expect(cell.text()).toContain('3/12 期')
    const progress = cell.findComponent(NProgress)
    expect(progress.exists()).toBe(true)
    expect(progress.props('percentage')).toBe(25)
  })

  it('移动档生命周期操作一击可达：暂停/取消为可见按钮且 ≥48px 触控目标，暂停走同一状态命令', async () => {
    setFakeMedia({ width: 600 })
    wirePlan()
    const wrapper = await mountView()
    for (const key of ['pause', 'cancel']) {
      const btn = wrapper.find(`[data-testid="op-${key}-i1"]`)
      expect(btn.exists(), `应存在可见的「${key}」按钮`).toBe(true)
      const el = btn.element as HTMLElement
      expect(el.style.minWidth).toBe('48px')
      expect(el.style.minHeight).toBe('48px')
    }
    await wrapper.find('[data-testid="op-pause-i1"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { input: { new_status: string } }).input.new_status === 'paused',
      ),
    ).toBe(true)
  })

  it('跨断点缩窗实时换列（十列 ⇄ 三列）', async () => {
    wirePlan()
    const wrapper = await mountView()
    expect((tableOf(wrapper).props('columns') as unknown[]).length).toBe(10)
    setFakeMedia({ width: 600 })
    await flushPromises()
    expect((tableOf(wrapper).props('columns') as unknown[]).length).toBe(3)
    setFakeMedia({ width: 1280 })
    await flushPromises()
    expect((tableOf(wrapper).props('columns') as unknown[]).length).toBe(10)
  })
})
