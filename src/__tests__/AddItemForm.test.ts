import { describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { findButton, findButtonByTestId } from './helpers/dom'
import { makeTransaction } from './factories'
import { mount, flushPromises } from '@vue/test-utils'
import { NMessageProvider } from 'naive-ui'
import { h } from 'vue'
import AddItemForm from '@/components/AddItemForm.vue'
import { formatAmount } from '@/utils/money'
import { refCurrencies } from './helpers/reference-stubs'
import type { Transaction } from '@ledger/types'

// 金额断言委托形态（issue #770）：期待值调同一 formatAmount 实现，格式规则唯一归属其专测
const cny = refCurrencies[0]


function createCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_item')
}

/** 挂载表单（NMessageProvider 包裹以满足 useMessage 注入），flush 后参考数据就绪。 */
async function mountForm(txn: Transaction) {
  const wrapper = mount(NMessageProvider, {
    slots: { default: () => h(AddItemForm, { transaction: txn }) },
  })
  await flushPromises()
  return wrapper
}

/** 弹窗布线：挂载即拉的物品列表空集（defaults 契约快照，中途重桩用例复用）。 */
const BASE_DEFAULTS = { list_items: [] }

beforeEach(() => {
  wireInvokeSeam({ defaults: BASE_DEFAULTS, overrides: { create_item: () => Promise.resolve('item-new') } })
})

describe('AddItemForm（加入物品确认弹窗，issue #119）', () => {
  it('预填：日期/成本/币种从交易只读带出（formatAmount），名称默认取交易备注', async () => {
    const wrapper = await mountForm(
      makeTransaction({
        id: 'txn-1',
        amount_cents: 599_900,
        amount_native_cents: 599_900,
        note: 'iPhone 15',
        date: '2026-01-15',
      }),
    )
    // 自动带出只读展示（文本节点，非输入控件）
    expect(wrapper.text()).toContain('购买日期')
    expect(wrapper.text()).toContain('2026-01-15')
    expect(wrapper.text()).toContain(formatAmount(599_900, cny))
    expect(wrapper.text()).toContain('CNY')
    // 名称默认 = 交易备注
    const nameInput = wrapper.find('input[placeholder="默认取交易备注，可微调"]')
    expect((nameInput.element as HTMLInputElement).value).toBe('iPhone 15')
  })

  it('备注为空时名称默认留空', async () => {
    const wrapper = await mountForm(
      makeTransaction({
        id: 'txn-1',
        amount_cents: 599_900,
        amount_native_cents: 599_900,
        note: null,
        date: '2026-01-15',
      }),
    )
    const nameInput = wrapper.find('input[placeholder="默认取交易备注，可微调"]')
    expect((nameInput.element as HTMLInputElement).value).toBe('')
  })

  it('确认创建：create_item 收到溯源必填的完整入参，emit created', async () => {
    const wrapper = await mountForm(
      makeTransaction({
        id: 'txn-1',
        amount_cents: 599_900,
        amount_native_cents: 599_900,
        note: 'iPhone 15',
        date: '2026-01-15',
      }),
    )
    const nameInput = wrapper.find('input[placeholder="默认取交易备注，可微调"]')
    await nameInput.setValue('iPhone 15 国行')
    await findButtonByTestId(wrapper, 'add-item-confirm').trigger('click')
    await flushPromises()
    expect(createCalls()).toHaveLength(1)
    const [, args] = createCalls()[0] as [string, { input: Record<string, unknown> }]
    expect(args.input).toEqual({
      name: 'iPhone 15 国行',
      purchase_date: '2026-01-15',
      total_cost_cents: 599_900,
      currency_code: 'CNY',
      note: null,
      purchase_transaction_id: 'txn-1',
    })
    expect(wrapper.findComponent(AddItemForm).emitted('created')).toHaveLength(1)
  })

  it('后端校验失败（重复创建/非 expense）：不 emit created（弹窗保持打开，错误经 message 可见）', async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        create_item: () =>
          Promise.reject(new Error('该购买交易已创建过物品，不能重复创建（溯源唯一）: txn-1')),
      },
    })
    const wrapper = await mountForm(
      makeTransaction({
        id: 'txn-1',
        amount_cents: 599_900,
        amount_native_cents: 599_900,
        note: 'iPhone 15',
        date: '2026-01-15',
      }),
    )
    await findButtonByTestId(wrapper, 'add-item-confirm').trigger('click')
    await flushPromises()
    expect(createCalls()).toHaveLength(1)
    // 失败 → 不关闭弹窗（视图只在 created 时关闭），错误信息由 message.error 呈现
    expect(wrapper.findComponent(AddItemForm).emitted('created')).toBeUndefined()
  })

  it('名称为空（备注为空且未填）时不提交', async () => {
    const wrapper = await mountForm(
      makeTransaction({
        id: 'txn-1',
        amount_cents: 599_900,
        amount_native_cents: 599_900,
        note: null,
        date: '2026-01-15',
      }),
    )
    await findButtonByTestId(wrapper, 'add-item-confirm').trigger('click')
    await flushPromises()
    expect(createCalls()).toHaveLength(0)
    expect(wrapper.findComponent(AddItemForm).emitted('created')).toBeUndefined()
  })

  it('点击取消 emit cancel（弹窗由视图关闭，不触发提交）', async () => {
    const wrapper = await mountForm(
      makeTransaction({
        id: 'txn-1',
        amount_cents: 599_900,
        amount_native_cents: 599_900,
        note: 'iPhone 15',
        date: '2026-01-15',
      }),
    )
    const cancelBtn = findButton(wrapper, '取消', { exact: true })!
    await cancelBtn.trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(AddItemForm).emitted('cancel')).toHaveLength(1)
    expect(createCalls()).toHaveLength(0)
  })
})
