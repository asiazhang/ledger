import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount, DOMWrapper } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import PolicyFormModal from '@/components/PolicyFormModal.vue'
import { makeAccount, makePolicy } from './factories'
import type { Account, Currency, Insurer, Policy } from '@/types'

const mockInvoke = vi.mocked(invoke)

// AppModal 内容 teleport 到 document.body：测试在 body 中查询/触发（同 PoliciesView 先例）。
enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

function bodyQuery(selector: string): HTMLElement | null {
  return document.body.querySelector(selector)
}

function formInput(testid: string): DOMWrapper<HTMLInputElement> {
  const modal = bodyQuery('[data-testid="policy-form-modal"]')!
  return new DOMWrapper(modal.querySelector(`[data-testid="${testid}"] input`)!)
}

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
]

const mockInsurers: Insurer[] = [
  { id: 'ins-1', name: '平安保险', is_deleted: false, created_at: '', updated_at: '', version: 1, device_id: 'test' },
]

const mockAccounts: Account[] = [
  makeAccount({ id: 'acc-1', name: '现金', type: 'cash' }),
]

/** 新建模式下打开的空保单（editing=null）。 */
const noPolicy: Policy | null = null

function setupInvoke() {
  mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
    if (cmd === 'list_insurers') return Promise.resolve(mockInsurers)
    if (cmd === 'list_policies') return Promise.resolve([])
    if (cmd === 'list_policy_stats') return Promise.resolve([])
    if (cmd === 'create_policy') {
      const { input } = args as { input: { policy_number: string } }
      return Promise.resolve(`policy-new-${input.policy_number}`)
    }
    if (cmd === 'create_scheduled_transaction') return Promise.resolve('plan-1')
    if (cmd === 'create_insurer') return Promise.reject(new Error('unexpected create_insurer'))
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

async function openCreateModal() {
  const wrapper = mount(PolicyFormModal, {
    props: { show: true, editing: noPolicy },
  })
  await flushPromises()
  return wrapper
}

/** 开启协议开关并填全协议字段（金额/账户；起始日默认今天已就绪）。 */
async function enableAgreement(wrapper: ReturnType<typeof mount>) {
  const toggle = new DOMWrapper(bodyQuery('[data-testid="policy-agreement-toggle"]')!)
  await toggle.trigger('click')
  await flushPromises()
  await formInput('policy-agreement-amount').setValue('3000')
  // 扣款账户：PinyinSelect 内部选择，emit update:value（同保司下拉先例；
  // teleport 不改变 vnode 层级，仍可从 wrapper 按定位找到组件）
  wrapper
    .findComponent('[data-testid="policy-agreement-account"]')
    .vm.$emit('update:value', 'acc-1')
  await flushPromises()
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  setupInvoke()
  localStorage.clear()
  await flushPromises()
})

describe('PolicyFormModal 缴费协议区（issue #362 / ADR-0051 决策 2；不挂商户 #713 / ADR-0082）', () => {
  it('协议区可折叠可选：开关默认关，字段组隐藏；开启后可见', async () => {
    const wrapper = await openCreateModal()
    const fields = bodyQuery('[data-testid="policy-agreement-fields"]')!
    expect(fields.style.display).toBe('none')
    const toggle = new DOMWrapper(bodyQuery('[data-testid="policy-agreement-toggle"]')!)
    await toggle.trigger('click')
    await flushPromises()
    expect(fields.style.display).not.toBe('none')
  })

  it('跳过协议区保存 = 趸交/缴清纯档案：只调 create_policy，不调 create_scheduled_transaction', async () => {
    const wrapper = await openCreateModal()
    wrapper
      .findComponent('[data-testid="policy-insurer"]')
      .vm.$emit('update:value', 'ins-1')
    await formInput('policy-number').setValue('P2026-300')
    await formInput('policy-product').setValue('车险')
    await wrapper
      .findComponent('[data-testid="policy-start-date"]')
      .vm.$emit('update:formatted-value', '2026-01-01')

    await new DOMWrapper(bodyQuery('[data-testid="policy-save"]')!).trigger('click')
    await flushPromises()

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_policy')).toBe(true)
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_scheduled_transaction'),
    ).toBe(false)
  })

  it('开启协议区保存：先建档再创建订阅形态协议，携带保单引用且不挂商户，备注带险种', async () => {
    const wrapper = await openCreateModal()
    wrapper
      .findComponent('[data-testid="policy-insurer"]')
      .vm.$emit('update:value', 'ins-1')
    await formInput('policy-number').setValue('P2026-301')
    await formInput('policy-product').setValue('重疾险')
    await wrapper
      .findComponent('[data-testid="policy-start-date"]')
      .vm.$emit('update:formatted-value', '2026-01-01')
    await enableAgreement(wrapper)

    await new DOMWrapper(bodyQuery('[data-testid="policy-save"]')!).trigger('click')
    await flushPromises()

    const createPolicyCall = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_policy')!
    expect(createPolicyCall).toBeTruthy()
    const planCall = mockInvoke.mock.calls.find(
      ([cmd]) => cmd === 'create_scheduled_transaction',
    )
    expect(planCall).toBeTruthy()
    expect(planCall![1]).toMatchObject({
      input: {
        kind: 'subscription',
        policy_id: 'policy-new-P2026-301',
        merchant_id: null,
        amount_cents: 300_000,
        currency_code: 'CNY',
        note: '重疾险',
        account_id: 'acc-1',
        recurrence_type: 'yearly',
        recurrence_interval: 1,
      },
    })
    // 先建档（拿 policy_id）再建协议（引用该 id）
    expect(createPolicyCallTrack()).toBeLessThan(
      mockInvoke.mock.calls.findIndex(([cmd]) => cmd === 'create_scheduled_transaction'),
    )
  })

  /** create_policy 在全部 invoke 调用中的序号。 */
  function createPolicyCallTrack(): number {
    return mockInvoke.mock.calls.findIndex(([cmd]) => cmd === 'create_policy')
  }

  it('开启协议区但金额非法：警告且不提交任何请求', async () => {
    const wrapper = await openCreateModal()
    wrapper
      .findComponent('[data-testid="policy-insurer"]')
      .vm.$emit('update:value', 'ins-1')
    await formInput('policy-number').setValue('P2026-302')
    await formInput('policy-product').setValue('重疾险')
    await wrapper
      .findComponent('[data-testid="policy-start-date"]')
      .vm.$emit('update:formatted-value', '2026-01-01')
    const toggle = new DOMWrapper(bodyQuery('[data-testid="policy-agreement-toggle"]')!)
    await toggle.trigger('click')
    await flushPromises()
    await formInput('policy-agreement-amount').setValue('0')

    await new DOMWrapper(bodyQuery('[data-testid="policy-save"]')!).trigger('click')
    await flushPromises()

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_policy')).toBe(false)
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_scheduled_transaction'),
    ).toBe(false)
  })

  it('开启协议区但未选扣款账户：警告且不提交任何请求', async () => {
    const wrapper = await openCreateModal()
    wrapper
      .findComponent('[data-testid="policy-insurer"]')
      .vm.$emit('update:value', 'ins-1')
    await formInput('policy-number').setValue('P2026-303')
    await formInput('policy-product').setValue('重疾险')
    await wrapper
      .findComponent('[data-testid="policy-start-date"]')
      .vm.$emit('update:formatted-value', '2026-01-01')
    const toggle = new DOMWrapper(bodyQuery('[data-testid="policy-agreement-toggle"]')!)
    await toggle.trigger('click')
    await flushPromises()
    await formInput('policy-agreement-amount').setValue('3000')

    await new DOMWrapper(bodyQuery('[data-testid="policy-save"]')!).trigger('click')
    await flushPromises()

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_policy')).toBe(false)
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_scheduled_transaction'),
    ).toBe(false)
  })
})

describe('PolicyFormModal 保单弹窗排版统一（issue #636 / spec #630）', () => {
  /** 断言弹窗卡片：宽度归 md 档（480px）+ 无边框（AppModal 默认，调用点不再显式声明）。
   *  弹窗卡片 teleport 到 body，本测试直接 mount 组件、body 上应恰有一张卡片
   *  （先例 PhysicalAssetsView.test.ts 的 visibleModalCard，此处无视图自有卡片故免过滤）。 */
  function expectModalCard(width: string) {
    const cards = [...document.querySelectorAll<HTMLElement>('.n-card')]
    expect(cards, '当前应恰有一个弹窗卡片').toHaveLength(1)
    expect(cards[0].style.width).toBe(width)
    expect(cards[0].classList.contains('n-card--bordered')).toBe(false)
  }

  it('表单弹窗卡片宽度归 md 档（480px）且默认无边框', async () => {
    await openCreateModal()
    expectModalCard('480px')
  })

  it('协议开关提示为表单下方段落式说明，无内联 opacity 挤占开关行', async () => {
    await openCreateModal()
    const modal = bodyQuery('[data-testid="policy-form-modal"]')!
    const hint = modal.querySelector<HTMLElement>('.form-hint')
    expect(hint?.textContent).toContain('订阅形态缴费协议')
    const toggleRow = modal
      .querySelector('[data-testid="policy-agreement-toggle"]')!
      .closest('.n-form-item')!
    expect(toggleRow.querySelector('span[style*="opacity"]')).toBeNull()
  })
})
