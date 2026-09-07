import { describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { mount, flushPromises, DOMWrapper } from '@vue/test-utils'
import PolicyAgreementSection from '@/components/PolicyAgreementSection.vue'
import { makePolicy } from './factories'
import { formatAmount } from '@/utils/money'
import { refCurrencies } from './helpers/reference-stubs'
import type {
  Policy,
  ScheduledTransactionDetail,
  ScheduledTransactionWithExt,
} from '@/types'
import { componentVm } from './helpers/component-vm'

// 金额断言委托形态（issue #770）：期待值调同一 formatAmount 实现，格式规则唯一归属其专测
const cny = refCurrencies[0]

const policy: Policy = makePolicy({ id: 'policy-1', insurer_id: 'ins-1', product_name: '重疾险' })

/** 协议行工厂：订阅形态 + 保单引用（分段历史断言用）。 */
function makeSegment(partial: {
  id: string
  status: ScheduledTransactionWithExt['core']['status']
  amount_cents: number
  created_at: string
}): ScheduledTransactionWithExt {
  return {
    core: {
      id: partial.id,
      kind: 'subscription',
      status: partial.status,
      account_id: 'acc-1',
      category_id: null,
      amount_cents: partial.amount_cents,
      currency_code: 'CNY',
      recurrence_type: 'yearly',
      recurrence_interval: 1,
      recurrence_day: null,
      start_date: '2026-01-01',
      note: '重疾险',
      created_at: partial.created_at,
      updated_at: partial.created_at,
      version: 1,
      device_id: 'test',
      is_deleted: false,
    },
    merchant_id: null,
    policy_id: 'policy-1',
    total_amount_cents: null,
    total_occurrences: null,
    to_account_id: null,
  }
}

let plans: ScheduledTransactionWithExt[]

function detailFor(planId: string): ScheduledTransactionDetail {
  return {
    core: plans.find((p) => p.core.id === planId)!.core,
    extension: { scheduled_transaction_id: planId, merchant_id: 'mer-1', policy_id: 'policy-1' },
    pending_occurrences: [],
    completed_occurrences: 0,
    completed_amount_cents: 0,
    occurrences: [
      {
        id: `occ-${planId}`,
        scheduled_transaction_id: planId,
        scheduled_date: '2027-01-01',
        status: 'pending',
        transaction_id: null,
        amount_cents: 300_000,
        created_at: '',
        updated_at: '',
        version: 1,
        device_id: 'test',
        is_deleted: false,
      },
    ],
  }
}

function setupInvoke() {
  wireInvokeSeam({
    defaults: { list_policies: [policy], create_scheduled_transaction: 'plan-new' },
    overrides: {
      list_scheduled_transactions: () => plans,
      get_scheduled_transaction_detail: (args) =>
        Promise.resolve(detailFor((args as { id: string }).id)),
      update_scheduled_transaction_status: () => Promise.resolve(),
    },
  })
}

function mountSection() {
  return mount(PolicyAgreementSection, { props: { policy }, attachTo: document.body })
}

function formButton(testid: string): DOMWrapper<HTMLButtonElement> {
  return new DOMWrapper(document.querySelector(`[data-testid="${testid}"]`)!)
}

function amountInput(): DOMWrapper<HTMLInputElement> {
  const form = document.querySelector('[data-testid="policy-agreement-form"]')!
  return new DOMWrapper(form.querySelector('[data-testid="policy-agreement-amount"] input')!)
}

/** 协议字段组内的扣款账户（组件实例 emit，非 teleport 直接挂载）。 */
async function selectAccount(wrapper: ReturnType<typeof mount>) {
  componentVm(wrapper.findComponent('[data-testid="policy-agreement-account"]')).$emit('update:value', 'acc-1')
  await flushPromises()
}

beforeEach(async () => {
  plans = []
  setupInvoke()
  await flushPromises()
})

describe('PolicyAgreementSection 缴费协议区（issue #362）', () => {
  it('展示协议历史：多段（含已取消）按创建先后排列（1 张保单 → 多段协议可展示）', async () => {
    plans = [
      makeSegment({ id: 'plan-old', status: 'cancelled', amount_cents: 300_000, created_at: '2026-01-01T00:00:00Z' }),
      makeSegment({ id: 'plan-new', status: 'active', amount_cents: 360_000, created_at: '2027-01-01T00:00:00Z' }),
    ]
    const wrapper = mountSection()
    await flushPromises()
    const table = document.querySelector('[data-testid="policy-agreement-segments"]')!
    expect(table.textContent).toContain(formatAmount(300_000, cny))
    expect(table.textContent).toContain(formatAmount(360_000, cny))
    expect(table.textContent).toContain('已取消')
    // 无活跃段判断不受取消段影响：存在活跃段 → 操作行为「改价」
    expect(wrapper.find('[data-testid="policy-agreement-rebuild-open"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="policy-agreement-add"]').exists()).toBe(false)
  })

  it('趸交/缴清（无协议）：显示空态与「添加缴费协议」入口', async () => {
    const wrapper = mountSection()
    await flushPromises()
    expect(wrapper.find('[data-testid="policy-agreement-add"]').exists()).toBe(true)
  })

  it('添加缴费协议：创建入参携带保单引用且不挂商户，备注带险种，成功后重拉历史', async () => {
    const wrapper = mountSection()
    await flushPromises()
    await wrapper.find('[data-testid="policy-agreement-add"]').trigger('click')
    await flushPromises()
    await amountInput().setValue('3000')
    await selectAccount(wrapper)

    await formButton('policy-agreement-submit').trigger('click')
    await flushPromises()

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_scheduled_transaction')
    expect(call).toBeTruthy()
    expect(call![1]).toMatchObject({
      input: {
        kind: 'subscription',
        policy_id: 'policy-1',
        merchant_id: null,
        amount_cents: 300_000,
        note: '重疾险',
      },
    })
    // 提交后重拉协议历史
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_scheduled_transactions').length).toBeGreaterThanOrEqual(2)
  })

  it('改价：先取消旧协议再按新金额重建，新段起始日预填下期扣款日并携带引用', async () => {
    plans = [makeSegment({ id: 'plan-1', status: 'active', amount_cents: 300_000, created_at: '2026-01-01T00:00:00Z' })]
    const wrapper = mountSection()
    await flushPromises()
    await wrapper.find('[data-testid="policy-agreement-rebuild-open"]').trigger('click')
    await flushPromises()
    await amountInput().setValue('3600')
    await selectAccount(wrapper)

    await formButton('policy-agreement-submit').trigger('click')
    await flushPromises()

    const cancelCallIdx = mockInvoke.mock.calls.findIndex(
      ([cmd]) => cmd === 'update_scheduled_transaction_status',
    )
    const createCallIdx = mockInvoke.mock.calls.findIndex(
      ([cmd]) => cmd === 'create_scheduled_transaction',
    )
    expect(cancelCallIdx).toBeGreaterThanOrEqual(0)
    expect(mockInvoke.mock.calls[cancelCallIdx][1]).toMatchObject({
      input: { id: 'plan-1', new_status: 'cancelled' },
    })
    expect(createCallIdx).toBeGreaterThan(cancelCallIdx)
    expect(mockInvoke.mock.calls[createCallIdx][1]).toMatchObject({
      input: {
        kind: 'subscription',
        policy_id: 'policy-1',
        amount_cents: 360_000,
        currency_code: 'CNY',
        recurrence_type: 'yearly',
        account_id: 'acc-1',
        start_date: '2027-01-01',
      },
    })
  })

  it('金额非法时提交：警告且不发起任何写入', async () => {
    plans = []
    const wrapper = mountSection()
    await flushPromises()
    await wrapper.find('[data-testid="policy-agreement-add"]').trigger('click')
    await flushPromises()
    await amountInput().setValue('0')
    await selectAccount(wrapper)

    await formButton('policy-agreement-submit').trigger('click')
    await flushPromises()

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_scheduled_transaction')).toBe(false)
  })
})
