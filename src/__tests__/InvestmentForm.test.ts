import { describe, it, expect, vi, beforeEach, beforeAll } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { NSelect } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import InvestmentForm from '@/components/InvestmentForm.vue'
import type { Account, Currency, Instrument } from '@/types'

const mockInvoke = vi.mocked(invoke)

// jsdom 不实现 scrollTo：naive-ui 打开虚拟滚动下拉时会调用，提前 polyfill 避免 unhandled rejection
beforeAll(() => {
  Element.prototype.scrollTo = () => {}
})

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1',
    name: '证券户',
    type: 'investment',
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

const mockInstruments: Instrument[] = [
  {
    id: 'ins-1',
    symbol: 'NVDA',
    name: '英伟达',
    type: 'stock',
    currency_code: 'CNY',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

/** 标的下拉 = 带 remote 搜索的那个 NSelect */
function instrumentSelect(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAllComponents(NSelect).find((s) => s.props('remote'))!
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
    if (cmd === 'list_insurers') return Promise.resolve([])
    if (cmd === 'list_instruments') return Promise.resolve({ items: [], total: 0 })
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  const store = useReferenceStore()
  await store.refresh()
})

describe('InvestmentForm.vue 移除「新增标的」入口（issue #152）', () => {
  it('不渲染「新增标的」切换按钮与内嵌建档表单', () => {
    const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
    // 无「新增标的」按钮
    const newBtn = wrapper.findAll('button').find((b) => b.text().includes('新增标的'))
    expect(newBtn).toBeUndefined()
    // 无内嵌建档表单的字样
    expect(wrapper.text()).not.toContain('保存标的')
    expect(wrapper.text()).not.toContain('新增标的')
  })

  it('标的无候选时下拉空态提示「未找到标的，可通过同步或 AI 导入新增」', async () => {
    const wrapper = mount(InvestmentForm, {
      props: { kind: 'buy', submitLabel: '记买入' },
      attachTo: document.body,
    })
    // 用户点开标的选择（此时无候选）→ 下拉菜单渲染空态文案
    await instrumentSelect(wrapper).find('.n-base-selection').trigger('click')
    await flushPromises()
    expect(document.body.textContent).toContain('未找到标的，可通过同步或 AI 导入新增')
  })

  it('标的搜索与选择行为不受影响：搜索触发 list_instruments、候选可选择', async () => {
    vi.useFakeTimers()
    try {
      const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
      const select = instrumentSelect(wrapper)
      // 用户在标的选择框输入 → 触发远程搜索（防抖 300ms）
      await select.find('input').setValue('NVDA')
      await vi.advanceTimersByTimeAsync(300)
      const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments')
      expect(calls).toHaveLength(1)
      const [, args] = calls[0] as [string, { filter: { search: string } }]
      expect(args.filter.search).toBe('NVDA')
      // 返回候选后可选择
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'list_instruments') return Promise.resolve({ items: mockInstruments, total: 1 })
        return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
      })
      await select.find('input').setValue('NVDA')
      await vi.advanceTimersByTimeAsync(300)
      await flushPromises()
      expect((select.props('options') as { value: string }[]).map((o) => o.value)).toEqual(['ins-1'])
      select.vm.$emit('update:value', 'ins-1')
      await flushPromises()
      expect(select.props('value')).toBe('ins-1')
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('InvestmentForm.vue 基金申赎形态（issue #302）', () => {
  const fundInstruments = [
    {
      id: 'ins-fund',
      symbol: '000123',
      name: '某混合基金',
      type: 'fund' as const,
      currency_code: 'CNY',
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
      version: 1,
      device_id: 'test',
      is_deleted: false,
      market: 'unknown' as const,
      invested: false,
      price_cents: null,
    },
  ]

  /** 挂载并选中基金标的（远程搜索 → 选中） */
  async function mountWithFundSelected(kind: 'buy' | 'sell') {
    const wrapper = mount(InvestmentForm, {
      props: { kind, submitLabel: kind === 'buy' ? '记买入' : '记卖出' },
    })
    const select = instrumentSelect(wrapper)
    vi.useFakeTimers()
    try {
      await select.find('input').setValue('某混合')
      await vi.advanceTimersByTimeAsync(300)
      await flushPromises()
    } finally {
      vi.useRealTimers()
    }
    select.vm.$emit('update:value', 'ins-fund')
    await flushPromises()
    return wrapper
  }

  /** 按 placeholder 找 NInputNumber 内层 input */
  function inputByPlaceholder(wrapper: ReturnType<typeof mount>, placeholder: string) {
    return wrapper.findAll('input').find((i) => i.attributes('placeholder') === placeholder)
  }

  it('选基金标的：金额可编辑（确认单权威）、份额标签、单价只读反算', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_instruments') return Promise.resolve({ items: fundInstruments, total: 1 })
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = await mountWithFundSelected('buy')
    expect(inputByPlaceholder(wrapper, '确认金额（以确认单为准）')).toBeDefined()
    expect(inputByPlaceholder(wrapper, '确认份额（以确认单为准）')).toBeDefined()
    const priceInput = inputByPlaceholder(wrapper, '由金额与份额反算')
    expect(priceInput).toBeDefined()
    expect(priceInput!.attributes('disabled')).toBeDefined()
    // 股票形态的占位不应出现
    expect(inputByPlaceholder(wrapper, '自动计算')).toBeUndefined()
  })

  it('未选标的时保持股票形态（金额只读展示、单价可编辑）', () => {
    const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
    expect(inputByPlaceholder(wrapper, '自动计算')).toBeDefined()
    expect(inputByPlaceholder(wrapper, '确认金额（以确认单为准）')).toBeUndefined()
    const priceInput = inputByPlaceholder(wrapper, '单价')
    expect(priceInput).toBeDefined()
    expect(priceInput!.attributes('disabled')).toBeUndefined()
  })
})

describe('InvestmentForm.vue 字段错误态（ADR-0058 / issue #416）', () => {
  /** 数量输入框（placeholder「数量」，股票形态；基金形态为「确认份额（以确认单为准）」） */
  function quantityInput(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('input').find((i) => i.attributes('placeholder') === '数量')!
  }

  /** 单价输入框（placeholder「单价」，非基金形态的权威输入） */
  function priceInput(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('input').find((i) => i.attributes('placeholder') === '单价')!
  }

  /** 输入框所属 NInput 根元素（错误态 class 挂载处，同 #415 先例） */
  function inputRoot(input: ReturnType<typeof quantityInput>) {
    const el = input.element.closest('.n-input')
    expect(el).not.toBeNull()
    return el as Element
  }

  function hasErrorStatus(input: ReturnType<typeof quantityInput>) {
    return inputRoot(input).classList.contains('n-input--error-status')
  }

  /** 保存按钮（按可见文案定位；数量/费用 NInputNumber 自带步进按钮，不可按序取） */
  function submitButton(wrapper: ReturnType<typeof mount>, label: string) {
    return wrapper.findAll('button').find((b) => b.text().includes(label))!
  }

  /** 经内层 NSelect 注入选择（0=币种，1=投资账户，2=标的） */
  function selectByIndex(wrapper: ReturnType<typeof mount>, index: number, value: string) {
    return wrapper.findAllComponents(NSelect)[index].vm.$emit('update:value', value)
  }

  function createCalls() {
    return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')
  }

  it('初始为空不红，保存按钮可点（不惩罚尚未输入）', () => {
    const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false)
    expect(hasErrorStatus(priceInput(wrapper))).toBe(false)
    expect(submitButton(wrapper, '记买入').attributes('disabled')).toBeUndefined()
  })

  it('数量输入解析失败文本（4.30发）即时红显、保存禁用、非法文本原样保留', async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
    await quantityInput(wrapper).setValue('4.30发')
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(true)
    expect(submitButton(wrapper, '记买入').attributes('disabled')).toBeDefined()
    expect((quantityInput(wrapper).element as HTMLInputElement).value).toBe('4.30发')
  })

  it('数量超四位小数（1.23456）即时红显', async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
    await quantityInput(wrapper).setValue('1.23456')
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(true)
  })

  it('单价超四位小数（1.23456）即时红显、保存禁用；四位小数（1.2345）合法不红', async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
    await priceInput(wrapper).setValue('1.23456')
    expect(hasErrorStatus(priceInput(wrapper))).toBe(true)
    expect(submitButton(wrapper, '记买入').attributes('disabled')).toBeDefined()
    await priceInput(wrapper).setValue('1.2345')
    expect(hasErrorStatus(priceInput(wrapper))).toBe(false)
    expect(submitButton(wrapper, '记买入').attributes('disabled')).toBeUndefined()
  })

  it('非法文本失焦不清空、红态持续；修正后红态解除、保存恢复可点', async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
    await quantityInput(wrapper).setValue('4.30发')
    await quantityInput(wrapper).trigger('blur')
    expect((quantityInput(wrapper).element as HTMLInputElement).value).toBe('4.30发')
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(true)
    await quantityInput(wrapper).setValue('100')
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false)
    expect(submitButton(wrapper, '记买入').attributes('disabled')).toBeUndefined()
  })

  it('清空后未失焦不红；失焦红；重新输入合法解除', async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
    await quantityInput(wrapper).setValue('12')
    await quantityInput(wrapper).setValue('')
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false)
    await quantityInput(wrapper).trigger('blur')
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(true)
    await quantityInput(wrapper).setValue('12')
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false)
  })

  it('保存尝试时空值红显兜底（数量+单价同红），不发起提交（格式类 toast 被红态取代）', async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
    await submitButton(wrapper, '记买入').trigger('click')
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(true)
    expect(hasErrorStatus(priceInput(wrapper))).toBe(true)
    expect(createCalls()).toHaveLength(0)
  })

  it('编辑弹窗合法回填（数量 100 / 单价 150）不显示红态、保存可点', () => {
    const editingTx = {
      id: 'txn-buy-1',
      kind: 'buy' as const,
      amount_cents: 15500,
      currency_code: 'CNY',
      amount_native_cents: 15500,
      account_id: 'acc-1',
      to_account_id: null,
      category_id: null,
      refund_of_transaction_id: null,
      note: null,
      date: '2026-01-10',
      created_at: '2026-01-10T01:00:00Z',
    }
    const editingTrade = {
      instrument_id: 'ins-1',
      symbol: 'NVDA',
      instrument_name: '英伟达',
      instrument_type: 'stock' as const,
      quantity: 100,
      price_cents: 1500000,
      fee_cents: 500,
    }
    const wrapper = mount(InvestmentForm, {
      props: { kind: 'buy', submitLabel: '记买入', editing: editingTx, trade: editingTrade },
    })
    expect((quantityInput(wrapper).element as HTMLInputElement).value).toBe('100')
    expect((priceInput(wrapper).element as HTMLInputElement).value).toBe('150')
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false)
    expect(hasErrorStatus(priceInput(wrapper))).toBe(false)
    expect(submitButton(wrapper, '保存修改').attributes('disabled')).toBeUndefined()
  })

  it('纯零/负数数量不红、保存可点、提交走业务类校验通道（不发起 create_transaction）', async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
    await selectByIndex(wrapper, 1, 'acc-1')
    await selectByIndex(wrapper, 2, 'ins-1')
    await quantityInput(wrapper).setValue('0')
    await priceInput(wrapper).setValue('10')
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false)
    expect(submitButton(wrapper, '记买入').attributes('disabled')).toBeUndefined()
    await submitButton(wrapper, '记买入').trigger('click')
    await flushPromises()
    expect(createCalls()).toHaveLength(0)
    // 负数可解析（非格式错误闭集），同走提交通道
    await quantityInput(wrapper).setValue('-5')
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false)
    await submitButton(wrapper, '记买入').trigger('click')
    await flushPromises()
    expect(createCalls()).toHaveLength(0)
  })

  it('基金形态：份额字段红态同规、单价为只读反算无输入面、修正后保存恢复', async () => {
    const fundInstruments = [
      {
        id: 'ins-fund',
        symbol: '000123',
        name: '某混合基金',
        type: 'fund' as const,
        currency_code: 'CNY',
        created_at: '2026-01-01T00:00:00Z',
        updated_at: '2026-01-01T00:00:00Z',
        version: 1,
        device_id: 'test',
        is_deleted: false,
        market: 'unknown' as const,
        invested: false,
        price_cents: null,
      },
    ]
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_instruments') return Promise.resolve({ items: fundInstruments, total: 1 })
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
    const select = instrumentSelect(wrapper)
    vi.useFakeTimers()
    try {
      await select.find('input').setValue('某混合')
      await vi.advanceTimersByTimeAsync(300)
      await flushPromises()
    } finally {
      vi.useRealTimers()
    }
    select.vm.$emit('update:value', 'ins-fund')
    await flushPromises()

    const sharesInput = wrapper.findAll('input').find((i) => i.attributes('placeholder') === '确认份额（以确认单为准）')!
    await sharesInput.setValue('4.30发')
    expect(hasErrorStatus(sharesInput)).toBe(true)
    expect(submitButton(wrapper, '记买入').attributes('disabled')).toBeDefined()
    // 基金单价为只读反算展示（disabled），无自由输入面即无红态口径
    const derivedInput = wrapper.findAll('input').find((i) => i.attributes('placeholder') === '由金额与份额反算')!
    expect(derivedInput.attributes('disabled')).toBeDefined()
    // 修正份额：红态解除、保存恢复可点（基金形态无单价错误态牵连）
    await sharesInput.setValue('987.6543')
    expect(hasErrorStatus(sharesInput)).toBe(false)
    expect(submitButton(wrapper, '记买入').attributes('disabled')).toBeUndefined()
  })
})

describe('InvestmentForm.vue 编辑模式（issue #180）', () => {
  const editingTx = {
    id: 'txn-buy-1',
    kind: 'buy' as const,
    amount_cents: 15500,
    currency_code: 'CNY',
    amount_native_cents: 15500,
    account_id: 'acc-1',
    to_account_id: null,
    category_id: null,
    refund_of_transaction_id: null,
    note: '建仓买入',
    date: '2026-01-10',
    created_at: '2026-01-10T01:00:00Z',
    updated_at: '2026-01-10T01:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  }

  const editingTrade = {
    instrument_id: 'ins-1',
    symbol: 'NVDA',
    instrument_name: '英伟达',
    instrument_type: 'stock' as const,
    quantity: 100,
    price_cents: 15000,
    fee_cents: 500,
  }

  it('编辑回填：标的候选直接显示 symbol · name（不依赖搜索），按钮文案「保存修改」', () => {
    const wrapper = mount(InvestmentForm, {
      props: { kind: 'buy', submitLabel: '记买入', editing: editingTx, trade: editingTrade },
    })
    const select = instrumentSelect(wrapper)
    expect(select.props('value')).toBe('ins-1')
    expect((select.props('options') as Array<{ label: string }>)[0].label).toBe('NVDA · 英伟达')
    expect(wrapper.text()).toContain('保存修改')
    expect(wrapper.text()).not.toContain('记买入')
  })

  it('编辑提交：触发 saved 事件（父层据此关窗），创建路径仍触发 created', async () => {
    const wrapper = mount(InvestmentForm, {
      props: { kind: 'buy', submitLabel: '记买入', editing: editingTx, trade: editingTrade },
    })
    mockInvoke.mockImplementationOnce((cmd: string) => {
      if (cmd === 'update_transaction') return Promise.resolve()
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await wrapper.findAll('button').find((b) => b.text().includes('保存修改'))!.trigger('click')
    await flushPromises()
    expect(wrapper.emitted('saved')).toHaveLength(1)
    expect(wrapper.emitted('created')).toBeUndefined()
  })
})
