import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount, flushPromises, enableAutoUnmount, DOMWrapper } from '@vue/test-utils'
import { NPopconfirm, NSelect, NDatePicker } from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { nextTick } from 'vue'
import { useItemsStore } from '@/stores/items'
import { applyLocale } from '@/i18n'
import ItemsView from '@/views/ItemsView.vue'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import type { Currency, ItemDailyCost, ItemInput, ItemWithDailyCost, Transaction } from '@/types'


const pushMock = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push: pushMock }),
}))

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

/** 自选参考日重算的 mock 返回（null = 模拟重算失败，issue #121）。 */
let calcResponse: ItemDailyCost | null = null

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
  stubReferenceInvoke({
    list_currencies: mockCurrencies,
    list_accounts: [],
    list_categories: [],
    list_insurers: [],
    list_merchants: [],
    list_transactions: (args?: Record<string, unknown>) => {
      const filter = (args as { filter?: { kind?: string } | null } | undefined)?.filter
      // 物品视图只拉支出交易（关联购买交易候选）；其他 kind 返回空
      return Promise.resolve({
        items: filter?.kind === 'expense' ? expenseTxs : [],
        total: filter?.kind === 'expense' ? expenseTxs.length : 0,
      })
    },
    list_items: () => Promise.resolve(itemList),
    calculate_item_cost: (args?: Record<string, unknown>) => {
      void (args as { id: string; referenceDate: string | null }).id
      if (calcResponse === null) return Promise.reject(new Error('重算失败'))
      return Promise.resolve(calcResponse)
    },
    update_item: (args?: Record<string, unknown>) => {
      const { id, input } = args as { id: string; input: { name: string } }
      itemList = itemList.map((it) => (it.id === id ? { ...it, ...input } : it))
      return Promise.resolve(null)
    },
    dispose_item: (args?: Record<string, unknown>) => {
      const { id, input } = args as {
        id: string
        input: { disposal_date: string; residual_value_cents: number | null }
      }
      itemList = itemList.map((it) =>
        it.id === id
          ? { ...it, status: 'disposed' as const, ...input, version: it.version + 1 }
          : it,
      )
      return Promise.resolve()
    },
    delete_item: (args?: Record<string, unknown>) => {
      const { id } = args as { id: string }
      itemList = itemList.filter((i) => i.id !== id)
      return Promise.resolve()
    },
  })
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  itemList = mockItems
  calcResponse = null
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

  it('不含新增物品表单，顶部常驻创建唯一入口提示（issue #207）', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()
    // 手动「新增物品」表单已移除
    expect(wrapper.text()).not.toContain('新增物品')
    expect(wrapper.find('input[placeholder="物品名称"]').exists()).toBe(false)
    // 提示条常驻顶部，说明创建方式
    const hint = wrapper.find('[data-testid="item-create-hint"]')
    expect(hint.exists()).toBe(true)
    expect(hint.text()).toContain('物品不支持直接新增')
    expect(hint.text()).toContain('「交易」页右键一笔支出交易')
    expect(hint.text()).toContain('加入物品')
  })

  it('点击提示条「去交易页」跳转交易视图（issue #207）', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()
    const btn = wrapper.find('[data-testid="item-go-transactions"]')
    expect(btn.exists()).toBe(true)
    expect(btn.text()).toContain('去交易页')
    await btn.trigger('click')
    expect(pushMock).toHaveBeenCalledWith({ name: 'transactions' })
  })

  it('列表空态显示创建引导文案（issue #207）', async () => {
    itemList = []
    const wrapper = mount(ItemsView)
    await flushPromises()
    const guide = wrapper.find('[data-testid="item-empty-guide"]')
    expect(guide.exists()).toBe(true)
    expect(guide.text()).toContain('「交易」页右键一笔支出交易')
    expect(guide.text()).toContain('加入物品')
  })

  it('英文界面：列表卡片与创建提示以英文渲染（issue #352）', async () => {
    await applyLocale('en-US')
    let text = ''
    let hintText = ''
    try {
      const wrapper = mount(ItemsView)
      await flushPromises()
      text = wrapper.text()
      hintText = wrapper.find('[data-testid="item-create-hint"]').text()
    } finally {
      await applyLocale('zh-CN')
      await nextTick()
    }
    expect(text).toContain('Items')
    expect(hintText).toContain('Items cannot be added directly')
    expect(text).toContain('Go to Transactions')
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

describe('ItemsView 关联购买交易（issue #119）：编辑弹窗换关语义', () => {
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
    // 手动新增表单已移除（#207）：弹窗内是唯一一处「关联购买交易」下拉
    const modalSelect = wrapper
      .findAllComponents(NSelect)
      .find((s) => s.props('placeholder') === '关联购买交易')
    expect(modalSelect, '编辑弹窗应有「关联购买交易」下拉').toBeTruthy()
    expect(modalSelect!.props('value')).toBe('tx-1')
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

describe('ItemsView 物品处置（issue #120）', () => {
  it('点处置打开弹窗：处置日期默认今天、残值为空，确认后 dispose_item 收到整数分入参', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()

    const disposeBtn = wrapper.findAll('button').find((b) => b.text() === '处置')
    expect(disposeBtn).toBeTruthy()
    await disposeBtn!.trigger('click')
    await flushPromises()

    const modal = bodyQuery('[data-testid="item-dispose-modal"]')
    expect(modal).not.toBeNull()
    // 处置日期默认今天（jsdom 下 NDatePicker 渲染的 input 值）
    const dateInput = modal!.querySelector('input')!
    expect(dateInput.value).toMatch(/^\d{4}-\d{2}-\d{2}$/)
    // 残值默认为空
    const residualInput = new DOMWrapper(
      modal!.querySelector('input[placeholder="残值（元，可选）"]')!,
    )
    expect((residualInput.element as HTMLInputElement).value).toBe('')

    await residualInput.setValue('200')
    const confirmBtn = [...modal!.querySelectorAll('button')].find(
      (b) => b.textContent === '确认处置',
    )
    confirmBtn!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await flushPromises()

    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'dispose_item')
    expect(calls).toHaveLength(1)
    const [, args] = calls[0]
    expect(args).toEqual({
      id: 'item-1',
      input: { disposal_date: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/), residual_value_cents: 20_000 },
    })
    // 重拉后列表展示已处置状态
    expect(wrapper.text()).toContain('已处置')
  })

  it('已处置物品点处置信息打开弹窗并预填处置字段，可修正后保存', async () => {
    itemList = [
      { ...mockItems[0], status: 'disposed', disposal_date: '2026-06-01', residual_value_cents: 10_000 },
    ]
    const wrapper = mount(ItemsView)
    await flushPromises()

    const editDisposeBtn = wrapper.findAll('button').find((b) => b.text() === '处置信息')
    expect(editDisposeBtn).toBeTruthy()
    await editDisposeBtn!.trigger('click')
    await flushPromises()

    const modal = bodyQuery('[data-testid="item-dispose-modal"]')!
    const residualInput = new DOMWrapper(
      modal.querySelector('input[placeholder="残值（元，可选）"]')!,
    )
    // 预填残值：10000 分 → 100（元）
    expect((residualInput.element as HTMLInputElement).value).toBe('100')

    await residualInput.setValue('300')
    const saveBtn = [...modal.querySelectorAll('button')].find((b) => b.textContent === '保存')
    saveBtn!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await flushPromises()

    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'dispose_item')
    expect(calls).toHaveLength(1)
    const [, args] = calls[0]
    expect((args as { input: { residual_value_cents: number } }).input.residual_value_cents).toBe(
      30_000,
    )
    expect(wrapper.text()).toContain('已处置 2026-06-01')
  })

  it('已处置物品详情展示处置日期与残值（formatAmount）', async () => {
    itemList = [
      { ...mockItems[0], status: 'disposed', disposal_date: '2026-06-01', residual_value_cents: 10_000 },
    ]
    const wrapper = mount(ItemsView)
    await flushPromises()

    const detailBtn = wrapper.findAll('button').find((b) => b.text() === '详情')
    await detailBtn!.trigger('click')
    await flushPromises()

    const modal = bodyQuery('[data-testid="item-detail-modal"]')
    expect(modal).not.toBeNull()
    expect(modal!.textContent).toContain('已处置')
    expect(modal!.textContent).toContain('2026-06-01')
    // 残值 10000 分 → ¥100（formatAmount）
    expect(modal!.textContent).toContain('¥100')
  })

  it('处置日期为空时提示且不调用 dispose_item', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()

    const disposeBtn = wrapper.findAll('button').find((b) => b.text() === '处置')
    await disposeBtn!.trigger('click')
    await flushPromises()

    const modal = bodyQuery('[data-testid="item-dispose-modal"]')!
    // 清空处置日期（直接置空底层输入）
    const dateInputEl = modal.querySelector('input') as HTMLInputElement
    const dateInput = new DOMWrapper(dateInputEl)
    await dateInput.setValue('')
    const confirmBtn = [...modal.querySelectorAll('button')].find(
      (b) => b.textContent === '确认处置',
    )
    confirmBtn!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await flushPromises()

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'dispose_item')).toBe(false)
    // 弹窗仍开着
    expect(bodyQuery('[data-testid="item-dispose-modal"]')).not.toBeNull()
  })
})

describe('ItemsView 自选参考日重算（issue #121）', () => {
  /** 打开第 1 件物品的详情弹窗并返回弹窗元素。 */
  async function openDetailModal(wrapper: ReturnType<typeof mount>) {
    const detailBtn = wrapper.findAll('button').find((b) => b.text() === '详情')
    await detailBtn!.trigger('click')
    await flushPromises()
    const modal = bodyQuery('[data-testid="item-detail-modal"]')
    expect(modal).not.toBeNull()
    return modal!
  }

  it('打开详情不自动重算；选择参考日后调用 calculate_item_cost 并展示重算结果', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()
    const modal = await openDetailModal(wrapper)

    // 未选参考日：不发起重算，展示列表快照口径
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'calculate_item_cost')).toBe(false)
    expect(modal.textContent).toContain('1000')

    calcResponse = { used_days: 2000, numerator_cents: 1_000_000, per_day_cents: 500 }
    const picker = wrapper
      .findAllComponents(NDatePicker)
      .find((s) => s.props('placeholder') === '自选参考日')
    expect(picker, '详情弹窗应有「自选参考日」选择器').toBeTruthy()
    picker!.vm.$emit('update:formatted-value', '2026-03-01')
    await flushPromises()

    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'calculate_item_cost')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toEqual({ id: 'item-1', referenceDate: '2026-03-01' })
    // 重算结果覆盖展示：2000 天；每天成本 500 分 → ¥5；分子 ¥1,0000
    const text = modal.textContent ?? ''
    expect(text).toContain('2000')
    expect(text).toContain('¥5')
    expect(text).toContain('¥1,0000')
  })

  it('清空参考日回退缺省口径（referenceDate 传 null）', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()
    await openDetailModal(wrapper)

    const picker = wrapper
      .findAllComponents(NDatePicker)
      .find((s) => s.props('placeholder') === '自选参考日')!
    calcResponse = { used_days: 2000, numerator_cents: 1_000_000, per_day_cents: 500 }
    picker.vm.$emit('update:formatted-value', '2026-03-01')
    await flushPromises()
    expect(modalText()).toContain('2000')

    // 清空 → 缺省口径（mock 返回与列表快照一致的天数）
    calcResponse = { used_days: 1000, numerator_cents: 1_000_000, per_day_cents: 1000 }
    picker.vm.$emit('update:formatted-value', null)
    await flushPromises()

    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'calculate_item_cost')
    expect(calls).toHaveLength(2)
    expect((calls[1][1] as { referenceDate: string | null }).referenceDate).toBeNull()
    expect(modalText()).toContain('1000')
  })

  it('重算失败提示且保留原值', async () => {
    const wrapper = mount(ItemsView)
    await flushPromises()
    const modal = await openDetailModal(wrapper)

    // calcResponse 保持 null → 重算 reject
    const picker = wrapper
      .findAllComponents(NDatePicker)
      .find((s) => s.props('placeholder') === '自选参考日')!
    picker.vm.$emit('update:formatted-value', '2026-03-01')
    await flushPromises()

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'calculate_item_cost')).toBe(true)
    // 原列表快照口径不变
    const text = modal.textContent ?? ''
    expect(text).toContain('1000')
    expect(text).toContain('¥10')
  })

  function modalText(): string {
    return bodyQuery('[data-testid="item-detail-modal"]')!.textContent ?? ''
  }
})
