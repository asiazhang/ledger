import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount, DOMWrapper } from '@vue/test-utils'
import { NPopconfirm } from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import PoliciesView from '@/views/PoliciesView.vue'
import PolicyFormModal from '@/components/PolicyFormModal.vue'
import { makePolicy } from './factories'
import type { Currency, Merchant, Policy } from '@/types'

const mockInvoke = vi.mocked(invoke)

// NModal 内容 teleport 到 document.body：测试在 body 中查询/触发（同 ItemsView 先例）。
enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

function bodyQuery(selector: string): HTMLElement | null {
  return document.body.querySelector(selector)
}

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
]

const mockMerchants: Merchant[] = [
  { id: 'mer-1', name: '平安保险', is_deleted: false, created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test' },
  { id: 'mer-2', name: '太平洋保险', is_deleted: false, created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test' },
]

function basePolicy(over: Partial<Policy> = {}): Policy {
  return makePolicy({ id: 'policy-1', ...over })
}

let policies: Policy[]

function setupInvoke() {
  mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve(mockMerchants)
    if (cmd === 'list_policies') {
      return Promise.resolve(policies.filter((p) => !p.is_deleted))
    }
    if (cmd === 'create_policy') {
      const { input } = args as { input: { policy_number: string; merchant_id: string } }
      const id = `policy-new-${input.policy_number}`
      policies = [
        ...policies,
        basePolicy({ id, ...input, coverage_amount_cents: null, coverage_currency_code: null, end_date: null }),
      ]
      return Promise.resolve(id)
    }
    if (cmd === 'create_merchant') {
      const { input } = args as { input: { name: string } }
      const id = `mer-new-${input.name}`
      mockMerchants.push({ id, name: input.name, is_deleted: false, created_at: '', updated_at: '', version: 1, device_id: 'test' })
      return Promise.resolve(id)
    }
    if (cmd === 'update_policy') {
      const { id, input } = args as { id: string; input: Partial<Policy> }
      policies = policies.map((p) => (p.id === id ? { ...p, ...input } : p))
      return Promise.resolve()
    }
    if (cmd === 'delete_policy') {
      const { id } = args as { id: string }
      policies = policies.map((p) => (p.id === id ? { ...p, is_deleted: true } : p))
      return Promise.resolve()
    }
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  policies = [basePolicy()]
  setupInvoke()
  localStorage.clear()
  // 参考数据（商户/币种选项）与保单 store 均为 self-init，提前预热
  await flushPromises()
})

type ViewWrapper = ReturnType<typeof mount>

/** 保存按钮在 teleported 弹窗内（document.body），经 body 查询驱动 */
function saveButton(): DOMWrapper<HTMLButtonElement> {
  return new DOMWrapper(bodyQuery('[data-testid="policy-save"]'))
}

function formInput(testid: string): DOMWrapper<HTMLInputElement> {
  const modal = bodyQuery('[data-testid="policy-form-modal"]')!
  return new DOMWrapper(modal.querySelector(`[data-testid="${testid}"] input`))
}

/** 弹窗内日期选择器：直接 emit formatted-value（同计划表单测试先例） */
function setDate(wrapper: ViewWrapper, testid: string, value: string | null) {
  wrapper
    .findComponent(`[data-testid="${testid}"]`)
    .vm.$emit('update:formatted-value', value)
}

describe('PoliciesView 保单列表（issue #360）', () => {
  it('渲染列表：保司名 / 险种 / 保单号 / 保障期间 / 状态 / 保额', async () => {
    const wrapper = mount(PoliciesView)
    await flushPromises()
    const text = wrapper.text()
    expect(text).toContain('平安保险') // 商户名经 merchantMap 解析
    expect(text).toContain('重疾险')
    expect(text).toContain('P2026-001')
    expect(text).toContain('2024-01-01 ~ 2036-01-01')
    expect(text).toContain('保障中')
    // 保额纯展示：30_000_000 分 → ¥30,0000（自带币种原样格式化，不折算）
    expect(text).toContain('¥30,0000')
  })

  it('止日为空显示「长期」；止日已过显示「已到期」', async () => {
    policies = [
      basePolicy({ id: 'p-1', end_date: null }),
      basePolicy({ id: 'p-2', end_date: '2020-01-01' }),
    ]
    const wrapper = mount(PoliciesView)
    await flushPromises()
    const text = wrapper.text()
    expect(text).toContain('2024-01-01 ~ 长期')
    expect(text).toContain('已到期')
    expect(text).toContain('保障中')
  })

  it('空列表显示建档引导', async () => {
    policies = []
    const wrapper = mount(PoliciesView)
    await flushPromises()
    expect(wrapper.find('[data-testid="policy-empty-guide"]').text()).toContain('暂无保单')
  })

  it('点击删除并确认：delete_policy 收到对应 id，列表移除（软删不进列表）', async () => {
    policies = [basePolicy(), basePolicy({ id: 'p-2', policy_number: 'P2026-002' })]
    const wrapper = mount(PoliciesView)
    await flushPromises()

    await wrapper.find('[data-testid="policy-delete-policy-1"]').trigger('click')
    await flushPromises()
    // 未确认前不删除
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'delete_policy')).toBe(false)

    // 确认（NPopconfirm 内容 teleport 到 body，直接对其组件 emit 正向点击）
    wrapper.findComponent(NPopconfirm).vm.$emit('positiveClick')
    await flushPromises()

    const deleteCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'delete_policy')
    expect(deleteCalls).toHaveLength(1)
    expect((deleteCalls[0][1] as { id: string }).id).toBe('policy-1')
    // 重拉后列表不再包含已删保单（软删不进列表）
    expect(wrapper.text()).not.toContain('P2026-001')
    expect(wrapper.text()).toContain('P2026-002')
  })
})

describe('PoliciesView 新建保单', () => {
  it('打开新建弹窗，完整填写并保存：create_policy 收到归一化入参，弹窗关闭', async () => {
    const wrapper = mount(PoliciesView)
    await flushPromises()
    await wrapper.find('[data-testid="policy-new"]').trigger('click')
    await flushPromises()
    expect(bodyQuery('[data-testid="policy-form-modal"]')).not.toBeNull()

    // 保司下拉选择既有商户（PinyinSelect 内部 NSelect，emit update:value）
    wrapper
      .findComponent('[data-testid="policy-merchant"]')
      .vm.$emit('update:value', 'mer-2')
    await flushPromises()
    await formInput('policy-number').setValue('P2026-100')
    await formInput('policy-product').setValue('医疗险')
    setDate(wrapper, 'policy-start-date', '2026-03-01')
    setDate(wrapper, 'policy-end-date', '2031-03-01')
    await formInput('policy-coverage').setValue('100000')
    await formInput('policy-note').setValue('百万医疗')

    await saveButton().trigger('click')
    await flushPromises()

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_policy')
    expect(call).toBeTruthy()
    expect(call![1]).toMatchObject({
      input: {
        merchant_id: 'mer-2',
        policy_number: 'P2026-100',
        product_name: '医疗险',
        start_date: '2026-03-01',
        end_date: '2031-03-01',
        coverage_amount_cents: 10_000_000,
        note: '百万医疗',
      },
    })
    // 创建成功后弹窗关闭
    expect(wrapper.findComponent(PolicyFormModal).emitted('update:show')).toContainEqual([false])
  })

  it('保司输入新名称：create_merchant 即建后携带新 id（同库同名一致）', async () => {
    const wrapper = mount(PoliciesView)
    await flushPromises()
    await wrapper.find('[data-testid="policy-new"]').trigger('click')
    await flushPromises()

    wrapper
      .findComponent('[data-testid="policy-merchant"]')
      .vm.$emit('update:value', '人保健康')
    await flushPromises()
    await formInput('policy-number').setValue('P2026-200')
    await formInput('policy-product').setValue('意外险')
    setDate(wrapper, 'policy-start-date', '2026-04-01')

    await saveButton().trigger('click')
    await flushPromises()

    const merchantCall = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_merchant')
    expect(merchantCall).toBeTruthy()
    expect((merchantCall![1] as { input: { name: string } }).input.name).toBe('人保健康')
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_policy')
    expect(call![1]).toMatchObject({ input: { merchant_id: 'mer-new-人保健康' } })
  })

  it('保司输入文本精确命中在用商户名：按名复用既有 id，不即建（全库同名一致）', async () => {
    const wrapper = mount(PoliciesView)
    await flushPromises()
    await wrapper.find('[data-testid="policy-new"]').trigger('click')
    await flushPromises()

    wrapper
      .findComponent('[data-testid="policy-merchant"]')
      .vm.$emit('update:value', '平安保险')
    await flushPromises()
    await formInput('policy-number').setValue('P2026-400')
    await formInput('policy-product').setValue('重疾险')
    setDate(wrapper, 'policy-start-date', '2026-06-01')

    await saveButton().trigger('click')
    await flushPromises()

    // 未发起 create_merchant，创建携带既有商户 id
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_merchant')).toBe(false)
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_policy')
    expect(call![1]).toMatchObject({ input: { merchant_id: 'mer-1' } })
  })

  it('止日留空保存为 null（长期/终身）；保额缺省币种一并存空', async () => {
    const wrapper = mount(PoliciesView)
    await flushPromises()
    await wrapper.find('[data-testid="policy-new"]').trigger('click')
    await flushPromises()

    wrapper
      .findComponent('[data-testid="policy-merchant"]')
      .vm.$emit('update:value', 'mer-1')
    await flushPromises()
    await formInput('policy-number').setValue('P2026-300')
    await formInput('policy-product').setValue('终身寿险')
    setDate(wrapper, 'policy-start-date', '2026-05-01')
    // 止日、保额均留空

    await saveButton().trigger('click')
    await flushPromises()

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_policy')
    expect(call![1]).toMatchObject({
      input: { end_date: null, coverage_amount_cents: null, coverage_currency_code: null },
    })
  })

  it('必填校验：保单号为空不发起 create_policy，弹窗不关', async () => {
    const wrapper = mount(PoliciesView)
    await flushPromises()
    await wrapper.find('[data-testid="policy-new"]').trigger('click')
    await flushPromises()

    wrapper
      .findComponent('[data-testid="policy-merchant"]')
      .vm.$emit('update:value', 'mer-1')
    await flushPromises()

    await saveButton().trigger('click')
    await flushPromises()

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_policy')).toBe(false)
    // 弹窗不关（内容不丢）
    expect(wrapper.findComponent(PolicyFormModal).emitted('update:show') ?? []).not.toContainEqual([false])
  })
})

describe('PoliciesView 编辑保单', () => {
  it('点编辑打开弹窗并预填当前行字段，保存走 update_policy 且携带 id 与保司', async () => {
    const wrapper = mount(PoliciesView)
    await flushPromises()

    await wrapper.find('[data-testid="policy-edit-policy-1"]').trigger('click')
    await flushPromises()
    expect(bodyQuery('[data-testid="policy-form-modal"]')).not.toBeNull()

    // 预填断言：保单号 / 险种 / 保额（30000000 分 → 300000 元）/ 起止日
    expect(formInput('policy-number').element.value).toBe('P2026-001')
    expect(formInput('policy-product').element.value).toBe('重疾险')
    expect(formInput('policy-coverage').element.value).toBe('300000')
    const pickers = wrapper
      .findComponent(PolicyFormModal)
      .findAllComponents('[data-testid="policy-start-date"], [data-testid="policy-end-date"]')
    expect(pickers.length).toBe(2)

    await formInput('policy-number').setValue('P-EDITED')
    await saveButton().trigger('click')
    await flushPromises()

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'update_policy')
    expect(call).toBeTruthy()
    expect(call![1]).toMatchObject({
      id: 'policy-1',
      input: { policy_number: 'P-EDITED', merchant_id: 'mer-1' },
    })
  })
})
