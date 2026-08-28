import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useItemsStore } from '@/stores/items'
import ItemsView from '@/views/ItemsView.vue'
import type { Currency, ItemInput, ItemWithDailyCost } from '@/types'

const mockInvoke = vi.mocked(invoke)

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

function setupInvoke() {
  mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_items') return Promise.resolve(itemList)
    if (cmd === 'create_item') {
      itemList = [...itemList, { ...mockItems[0], id: 'item-new', name: '键盘' }]
      void args
      return Promise.resolve('item-new')
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
})
