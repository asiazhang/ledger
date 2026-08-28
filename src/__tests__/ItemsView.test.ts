import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount, DOMWrapper } from '@vue/test-utils'
import { NPopconfirm, NSelect } from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useItemsStore } from '@/stores/items'
import ItemsView from '@/views/ItemsView.vue'
import type { Currency, ItemInput, ItemWithDailyCost, Transaction } from '@/types'

const mockInvoke = vi.mocked(invoke)

// NModal 内容 teleport 到 document.body：测试在 body 中查询/触发（同 InstrumentBrowser 先例）。
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

const mockItems: ItemWithDailyCost[] = [
  {
    id: 'item-1',
    name: '手机',
    purchase_date: '2025-01-01',
    total_cost_cents: 1_000_000,
    currency_code: 'CNY',
    cost_native_cents: 1_000_000,
    status: 'in_use',
    disposal_date: null,
    residual_value_cents: null,
    purchase_transaction_id: null,
    note: null,
    created_at: '2025-01-01T00:00:00Z',
    updated_at: '2025-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    used_days: 1000,
    per_day_cents: 1000,
  },
  {
    id: 'item-2',
    name: '显示器',
    purchase_date: '2026-01-01',
    total_cost_cents: 123_450,
    currency_code: 'USD',
    cost_native_cents: 890_000,
    status: 'in_use',
    disposal_date: null,
    residual_value_cents: null,
    purchase_transaction_id: null,
    note: null,
    created_at: '2026-01-02T00:00:00Z',
    updated_at: '2026-01-02T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    used_days: 30,
    per_day_cents: 4115,
  },
]

let itemList: ItemWithDailyCost[]

/** 可供关联的支出交易（后端 list_transactions kind=expense 过滤）。 */
const mockExpenseTxs: Transaction[] = [
  {
    id: 'tx-1',
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
  },
  {
    id: 'tx-2',
    kind: 'expense',
    amount_cents: 100_000,
    currency_code: 'CNY',
    amount_native_cents: 100_000,
    account_id: 'acc-1',
    to_account_id: null,
    category_id: null,
    refund_of_transaction_id: null,
    note: '显示器',
    date: '2026-02-20',
    created_at: '2026-02-20T00:00:00Z',
    updated_at: '2026-02-20T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

function setupInvoke(expenseTxs: Transaction[] = mockExpenseTxs) {
  mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_transactions') {
      const filter = (args as { filter?: { kind?: string } | null }).filter
      // 物品视图只拉支出交易（关联购买交易候选）；其他 kind 返回空
      return Promise.resolve({
        items: filter?.kind === 'expense' ? expenseTxs : [],
        total: filter?.kind === 'expense' ? expenseTxs.length : 0,
      })
    }
    if (cmd === 'list_items') return Promise.resolve(itemList)
    if (cmd === 'create_item') {
      itemList = [...itemList, { ...mockItems[0], id: 'item-new', name: '键盘' }]
      void args
      return Promise.resolve('item-new')
    }
    if (cmd === 'update_item') {
      const { id, input } = args as { id: string; input: { name: string } }
      itemList = itemList.map((it) => (it.id === id ? { ...it, ...input } : it))
      return Promise.resolve(null)
    }
    if (cmd === 'delete_item') {
      const { id } = args as { id: string }
      itemList = itemList.filter((i) => i.id !== id)
      return Promise.resolve()
    }
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  itemList = mockItems
  setupInvoke()
  localStorage.clear()
  // 参考数据（币种选项）与物品 store 均为 self-init，提前预热
  await flushPromises()
})

describe('ItemsView 物品列表', () => {
  it('渲染物品列表：名称 / 购买日期 / 总成本（formatAmount）/ 已用天数 / 每天成本', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()
    const html = wrapper.text()
    expect(html).toContain('手机')
    expect(html).toContain('显示器')
    // 总成本按原始币种 formatAmount（万分位分组：10000 元 → 1,0000）
    expect(html).toContain('¥1,0000')
    expect(html).toContain('$1234.5')
    expect(html).toContain('1000')
    expect(html).toContain('30')
    // 每天成本 formatAmount：1000 分/天 → ¥10；4115 分/天 → $41.15
    expect(html).toContain('¥10')
    expect(html).toContain('$41.15')
  })

  it('含创建入口（新增物品表单）', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()
    expect(wrapper.text()).toContain('新增物品')
  })

  it('填写名称与成本后创建：create_item 收到整数分入参，列表自动出现新物品', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()

    const nameInput = wrapper.find('input[placeholder="物品名称"]')
    expect(nameInput.exists()).toBe(true)
    await nameInput.setValue('键盘')
    const costInput = wrapper.find('input[placeholder="总成本（元）"]')
    await costInput.setValue('299')
    const noteInput = wrapper.find('input[placeholder="品牌 / 型号 / 渠道（可选）"]')
    await noteInput.setValue('HHKB')

    // 购买日期默认今天，无需交互；币种默认 CNY
    const createBtn = wrapper.findAll('button').find((b) => b.text() === '创建')
    expect(createBtn).toBeTruthy()
    await createBtn!.trigger('click')
    await flushPromises()

    const createCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_item')
    expect(createCalls).toHaveLength(1)
    const [, args] = createCalls[0]
    const input = (args as { input: ItemInput }).input
    expect(input.name).toBe('键盘')
    expect(input.total_cost_cents).toBe(29_900)
    expect(input.currency_code).toBe('CNY')
    expect(input.note).toBe('HHKB')
    expect(input.purchase_date).toMatch(/^\d{4}-\d{2}-\d{2}$/)

    // 创建成功后重拉：新物品出现在列表
    expect(wrapper.text()).toContain('键盘')
  })

  it('名称为空时提示且不调用 create_item', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()
    const createBtn = wrapper.findAll('button').find((b) => b.text() === '创建')
    await createBtn!.trigger('click')
    await flushPromises()
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'create_item')).toBe(false)
  })

  it('点击删除并确认：delete_item 收到对应 id，列表移除该物品', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()

    const deleteBtn = wrapper.findAll('button').find((b) => b.text() === '删除')
    expect(deleteBtn).toBeTruthy()
    await deleteBtn!.trigger('click')
    await flushPromises()
    // 未确认前不删除
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'delete_item')).toBe(false)

    // 确认（NPopconfirm 内容 teleport 到 body，直接对其组件 emit 正向点击）
    wrapper.findComponent(NPopconfirm).vm.$emit('positiveClick')
    await flushPromises()

    const deleteCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'delete_item')
    expect(deleteCalls).toHaveLength(1)
    expect((deleteCalls[0][1] as { id: string }).id).toBe('item-1')
    // 重拉后列表不再包含已删物品
    expect(wrapper.text()).not.toContain('手机')
    expect(wrapper.text()).toContain('显示器')
  })
})

describe('ItemsView 物品编辑（issue #117）', () => {
  it('点编辑打开弹窗并预填当前行字段', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()

    const editBtn = wrapper.findAll('button').find((b) => b.text() === '编辑')
    expect(editBtn).toBeTruthy()
    await editBtn!.trigger('click')
    await flushPromises()

    const modal = bodyQuery('[data-testid="item-edit-modal"]')
    expect(modal).not.toBeNull()
    const nameInput = new DOMWrapper(modal!.querySelector('input[placeholder="物品名称"]'))
    expect((nameInput.element as HTMLInputElement).value).toBe('手机')
    // 总成本预填：100000 分 → 10000（元）
    const costInput = new DOMWrapper(modal!.querySelector('input[placeholder="总成本（元）"]'))
    expect((costInput.element as HTMLInputElement).value).toBe('10000')
  })

  it('修改后保存：update_item 收到按 id 与整数分入参，列表重拉可见新名称', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()

    const editBtn = wrapper.findAll('button').find((b) => b.text() === '编辑')
    await editBtn!.trigger('click')
    await flushPromises()

    const modal = bodyQuery('[data-testid="item-edit-modal"]')!
    await new DOMWrapper(modal.querySelector('input[placeholder="物品名称"]')).setValue('手机 Pro')
    await new DOMWrapper(modal.querySelector('input[placeholder="总成本（元）"]')).setValue('12000')
    await new DOMWrapper(
      modal.querySelector('input[placeholder="品牌 / 型号 / 渠道（可选）"]'),
    ).setValue('顶配')

    const saveBtn = [...modal.querySelectorAll('button')].find((b) => b.textContent === '保存')
    saveBtn!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await flushPromises()

    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'update_item')
    expect(calls).toHaveLength(1)
    const [, args] = calls[0]
    const { id, input } = args as { id: string; input: ItemInput }
    expect(id).toBe('item-1')
    expect(input.name).toBe('手机 Pro')
    expect(input.total_cost_cents).toBe(1_200_000)
    // 币种不可改：沿用行内币种
    expect(input.currency_code).toBe('CNY')
    expect(input.note).toBe('顶配')
    expect(input.purchase_date).toBe('2025-01-01')

    // mock 已更新列表 → 重拉后新名称可见，弹窗关闭
    expect(wrapper.text()).toContain('手机 Pro')
    await new Promise((r) => setTimeout(r, 300))
    // jsdom 不派发 transitionend：NModal 退场后仅隐藏不卸载，断言 display:none 即已关闭
    const modalEl = bodyQuery('[data-testid="item-edit-modal"]') as HTMLElement | null
    expect(modalEl === null || modalEl.style.display === 'none').toBe(true)
  })

  it('编辑弹窗名称清空时提示且不调用 update_item', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()

    const editBtn = wrapper.findAll('button').find((b) => b.text() === '编辑')
    await editBtn!.trigger('click')
    await flushPromises()

    const modal = bodyQuery('[data-testid="item-edit-modal"]')!
    await new DOMWrapper(modal.querySelector('input[placeholder="物品名称"]')).setValue('  ')
    const saveBtn = [...modal.querySelectorAll('button')].find((b) => b.textContent === '保存')
    saveBtn!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await flushPromises()

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'update_item')).toBe(false)
    // 弹窗仍开着（未保存成功）
    expect(bodyQuery('[data-testid="item-edit-modal"]')).not.toBeNull()
  })
})

describe('ItemsView 物品详情（issue #117）', () => {
  it('点详情打开弹窗并展示成本分解：分子 ÷ 天数 = 每天成本', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()

    const detailBtn = wrapper.findAll('button').find((b) => b.text() === '详情')
    expect(detailBtn).toBeTruthy()
    await detailBtn!.trigger('click')
    await flushPromises()

    const modal = bodyQuery('[data-testid="item-detail-modal"]')
    expect(modal).not.toBeNull()
    const text = modal!.textContent ?? ''
    expect(text).toContain('手机')
    // 总成本（原始币种 formatAmount：10000 元 → ¥1,0000）
    expect(text).toContain('¥1,0000')
    // 成本分解：分子 ¥1,0000 ÷ 1000 天 = 每天成本 ¥10
    expect(text).toContain('1000')
    expect(text).toContain('¥10')
    expect(text).toContain('每天成本分解')
  })

  it('详情展示备注与购买日期', async () => {
    itemList = [itemList[0]]
    itemList[0] = { ...itemList[0], note: 'HHKB', purchase_date: '2025-06-01' }
    const wrapper = mount(ItemsView)
    await flushPromises()

    const detailBtn = wrapper.findAll('button').find((b) => b.text() === '详情')
    await detailBtn!.trigger('click')
    await flushPromises()

    const text = bodyQuery('[data-testid="item-detail-modal"]')!.textContent ?? ''
    expect(text).toContain('HHKB')
    expect(text).toContain('2025-06-01')
  })
})

describe('ItemsView 关联购买交易（issue #119）', () => {
  /** 创建表单里的「关联购买交易」下拉（按 placeholder 区分于币种下拉）。 */
  function linkSelect(wrapper: ReturnType<typeof mount>) {
    const sel = wrapper
      .findAllComponents(NSelect)
      .find((s) => s.props('placeholder') === '关联购买交易')
    expect(sel, '创建表单应有「关联购买交易」下拉').toBeTruthy()
    return sel!
  }

  it('未关联时创建入参 purchase_transaction_id 为 null', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()

    await wrapper.find('input[placeholder="物品名称"]').setValue('键盘')
    await wrapper.find('input[placeholder="总成本（元）"]').setValue('299')
    const createBtn = wrapper.findAll('button').find((b) => b.text() === '创建')
    await createBtn!.trigger('click')
    await flushPromises()

    const input = (mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_item')[0][1] as {
      input: ItemInput
    }).input
    expect(input.purchase_transaction_id).toBeNull()
  })

  it('选择关联交易后自动带出日期与成本（禁用手改），创建入参携带溯源 id', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()

    // 选交易前成本/日期可编辑
    const costInput = wrapper.find('input[placeholder="总成本（元）"]')
    expect(costInput.attributes('disabled')).toBeUndefined()

    await wrapper.find('input[placeholder="物品名称"]').setValue('iPhone')
    linkSelect(wrapper).vm.$emit('update:value', 'tx-1')
    await flushPromises()

    // 自动带出：成本表单值被交易金额覆盖（599900 分 → 5999 元），且禁用手改
    const costAfter = wrapper.find('input[placeholder="总成本（元）"]')
    expect((costAfter.element as HTMLInputElement).value).toBe('5999')
    expect(costAfter.attributes('disabled')).toBeDefined()

    const createBtn = wrapper.findAll('button').find((b) => b.text() === '创建')
    await createBtn!.trigger('click')
    await flushPromises()

    const input = (mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_item')[0][1] as {
      input: ItemInput
    }).input
    expect(input.purchase_transaction_id).toBe('tx-1')
    // 日期与成本为交易带出值（即使表单曾默认今天/被改过）
    expect(input.purchase_date).toBe('2026-01-15')
    expect(input.total_cost_cents).toBe(599_900)
    expect(input.currency_code).toBe('CNY')
  })

  it('编辑弹窗预填既有关联且可手动调成本，换关后锁定并携带新溯源 id', async () => {
    itemList = [
      { ...itemList[0], purchase_transaction_id: 'tx-1', purchase_date: '2026-01-15', total_cost_cents: 599_900 },
    ]
    const wrapper = mount(ItemsView)
    await flushPromises()

    const editBtn = wrapper.findAll('button').find((b) => b.text() === '编辑')
    await editBtn!.trigger('click')
    await flushPromises()

    const modal = bodyQuery('[data-testid="item-edit-modal"]')!
    // 弹窗内的关联下拉 = 同 placeholder 的第 2 个（第 1 个在创建表单）
    const linkSelects = wrapper
      .findAllComponents(NSelect)
      .filter((s) => s.props('placeholder') === '关联购买交易')
    const modalSelect = linkSelects[linkSelects.length - 1]
    expect(modalSelect.props('value')).toBe('tx-1')
    // 维持既有关联：成本可手动编辑（后续花费累加到总成本，用户故事 8）
    const costInput = modal.querySelector('input[placeholder="总成本（元）"]') as HTMLInputElement
    expect(costInput.value).toBe('5999')
    expect(costInput.disabled).toBe(false)

    // 换关到 tx-2 → 自动带出新日期与成本并锁定（后端将重新带出覆盖）
    modalSelect.vm.$emit('update:value', 'tx-2')
    await flushPromises()
    const costAfterSwitch = modal.querySelector(
      'input[placeholder="总成本（元）"]',
    ) as HTMLInputElement
    expect(costAfterSwitch.value).toBe('1000')
    expect(costAfterSwitch.disabled).toBe(true)

    await new DOMWrapper(modal.querySelector('input[placeholder="物品名称"]')).setValue('显示器 4K')
    const saveBtn = [...modal.querySelectorAll('button')].find((b) => b.textContent === '保存')
    saveBtn!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await flushPromises()

    const saved = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'update_item')[0][1] as {      id: string
      input: ItemInput
    }
    expect(saved.id).toBe('item-1')
    expect(saved.input.purchase_transaction_id).toBe('tx-2')
    expect(saved.input.purchase_date).toBe('2026-02-20')
    expect(saved.input.total_cost_cents).toBe(100_000)
  })

  it('详情展示关联购买交易溯源', async () => {
    itemList = [{ ...itemList[0], purchase_transaction_id: 'tx-1' }]
    const wrapper = mount(ItemsView)
    await flushPromises()

    const detailBtn = wrapper.findAll('button').find((b) => b.text() === '详情')
    await detailBtn!.trigger('click')
    await flushPromises()

    const text = bodyQuery('[data-testid="item-detail-modal"]')!.textContent ?? ''
    expect(text).toContain('关联购买交易')
  })
})
