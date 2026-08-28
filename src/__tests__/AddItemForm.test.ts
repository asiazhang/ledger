import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { NMessageProvider } from 'naive-ui'
import { h } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import AddItemForm from '@/components/AddItemForm.vue'
import type { Transaction } from '@/types'

const mockInvoke = vi.mocked(invoke)

enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

function makeTxn(overrides: Partial<Transaction> = {}): Transaction {
  return {
    id: 'txn-1',
    kind: 'expense',
    amount_cents: 599_900,
    currency_code: 'CNY',
    amount_native_cents: 599_900,
    account_id: 'acc-1',
    to_account_id: null,
    category_id: null,
    refund_of_transaction_id: null,
    note: 'iPhone 15',
    date: '2026-01-15',
    created_at: '2026-01-15T00:00:00Z',
    updated_at: '2026-01-15T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...overrides,
  }
}

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

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies')
      return Promise.resolve([{ code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 }])
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_items') return Promise.resolve([])
    if (cmd === 'create_item') return Promise.resolve('item-new')
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
})

describe('AddItemForm（加入物品确认弹窗，issue #119）', () => {
  it('预填：日期/成本/币种从交易只读带出（formatAmount），名称默认取交易备注', async () => {
    const wrapper = await mountForm(makeTxn())
    // 自动带出只读展示（文本节点，非输入控件）
    expect(wrapper.text()).toContain('购买日期')
    expect(wrapper.text()).toContain('2026-01-15')
    expect(wrapper.text()).toContain('¥5999')
    expect(wrapper.text()).toContain('CNY')
    // 名称默认 = 交易备注
    const nameInput = wrapper.find('input[placeholder="默认取交易备注，可微调"]')
    expect((nameInput.element as HTMLInputElement).value).toBe('iPhone 15')
  })

  it('备注为空时名称默认留空', async () => {
    const wrapper = await mountForm(makeTxn({ note: null }))
    const nameInput = wrapper.find('input[placeholder="默认取交易备注，可微调"]')
    expect((nameInput.element as HTMLInputElement).value).toBe('')
  })

  it('确认创建：create_item 收到溯源必填的完整入参，emit created', async () => {
    const wrapper = await mountForm(makeTxn())
    const nameInput = wrapper.find('input[placeholder="默认取交易备注，可微调"]')
    await nameInput.setValue('iPhone 15 国行')
    await wrapper.find('button[data-testid="add-item-confirm"]').trigger('click')
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
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies')
        return Promise.resolve([{ code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 }])
      if (cmd === 'list_accounts') return Promise.resolve([])
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_items') return Promise.resolve([])
      if (cmd === 'create_item')
        return Promise.reject(new Error('该购买交易已创建过物品，不能重复创建（溯源唯一）: txn-1'))
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = await mountForm(makeTxn())
    await wrapper.find('button[data-testid="add-item-confirm"]').trigger('click')
    await flushPromises()
    expect(createCalls()).toHaveLength(1)
    // 失败 → 不关闭弹窗（视图只在 created 时关闭），错误信息由 message.error 呈现
    expect(wrapper.findComponent(AddItemForm).emitted('created')).toBeUndefined()
  })

  it('名称为空（备注为空且未填）时不提交', async () => {
    const wrapper = await mountForm(makeTxn({ note: null }))
    await wrapper.find('button[data-testid="add-item-confirm"]').trigger('click')
    await flushPromises()
    expect(createCalls()).toHaveLength(0)
    expect(wrapper.findComponent(AddItemForm).emitted('created')).toBeUndefined()
  })

  it('点击取消 emit cancel（弹窗由视图关闭，不触发提交）', async () => {
    const wrapper = await mountForm(makeTxn())
    const cancelBtn = wrapper.findAll('button').find((b) => b.text() === '取消')!
    await cancelBtn.trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(AddItemForm).emitted('cancel')).toHaveLength(1)
    expect(createCalls()).toHaveLength(0)
  })
})
