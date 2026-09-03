import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import RefundForm from '@/components/RefundForm.vue'
import type { Account, Category, Currency, Transaction } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1', name: '现金', type: 'cash', currency_code: 'CNY',
    initial_balance_cents: 0, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
]

const mockCategories: Category[] = [
  {
    id: 'cat-1', name: '餐饮', kind: 'expense', parent_id: null,
    icon: null, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
]

/** 行内退款模式的固定原交易（¥30 支出） */
const fixedTx: Transaction = {
  id: 'txn-001',
  kind: 'expense',
  amount_cents: 3000,
  currency_code: 'CNY',
  amount_native_cents: 3000,
  account_id: 'acc-1',
  to_account_id: null,
  category_id: 'cat-1',
  merchant_id: null,
  refund_of_transaction_id: null,
  note: null,
  date: '2026-01-01',
  created_at: '2026-01-01T00:00:00Z',
}

describe('RefundForm.vue', () => {
  beforeEach(async () => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve(mockCategories)
      if (cmd === 'list_merchants') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    // Pre-load store so components have data
    const store = useReferenceStore()
    await store.refresh()
  })

  it('挂载并显示提交按钮文本', () => {
    const wrapper = mount(RefundForm)
    expect(wrapper.text()).toContain('记退款')
  })

  // ---- 字段错误态（ADR-0058 / issue #415，行为接缝）：断言用户可见状态——
  // 输入的错误态呈现（n-input--error-status）与保存按钮可用性，不断言内部状态变量。
  // 红态时机路径与支出/收入形态（#414 先例）、转账形态（#415）完全一致。
  /** 退款金额输入框（placeholder「退款金额」） */
  function amountInput(wrapper: ReturnType<typeof mount>) {
    return wrapper.find('input[placeholder="退款金额"]')
  }

  /** 金额输入的 NInput 根元素（错误态 class 挂载处） */
  function amountInputRoot(wrapper: ReturnType<typeof mount>) {
    const el = amountInput(wrapper).element.closest('.n-input')
    expect(el).not.toBeNull()
    return el as Element
  }

  function hasErrorStatus(wrapper: ReturnType<typeof mount>) {
    return amountInputRoot(wrapper).classList.contains('n-input--error-status')
  }

  /** 保存按钮（表单内唯一按钮） */
  function submitButton(wrapper: ReturnType<typeof mount>) {
    return wrapper.find('button')
  }

  function createCalls() {
    return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')
  }

  describe('字段错误态（ADR-0058 / #415）', () => {
    it('初始为空不红，保存按钮可点（搜索选择模式）', () => {
      const wrapper = mount(RefundForm)
      expect(hasErrorStatus(wrapper)).toBe(false)
      expect(submitButton(wrapper).attributes('disabled')).toBeUndefined()
    })

    it('输入解析失败文本（4.30发）即时红显、保存禁用、非法文本原样保留', async () => {
      const wrapper = mount(RefundForm)
      await amountInput(wrapper).setValue('4.30发')
      expect(hasErrorStatus(wrapper)).toBe(true)
      expect(submitButton(wrapper).attributes('disabled')).toBeDefined()
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe('4.30发')
    })

    it('多个小数点（4.3.0）与超两位小数（4.305）同样即时红显', async () => {
      const wrapper = mount(RefundForm)
      await amountInput(wrapper).setValue('4.3.0')
      expect(hasErrorStatus(wrapper)).toBe(true)
      await amountInput(wrapper).setValue('4.305')
      expect(hasErrorStatus(wrapper)).toBe(true)
    })

    it('非法文本失焦不清空、红态持续；修正后红态解除、保存恢复可点', async () => {
      const wrapper = mount(RefundForm)
      await amountInput(wrapper).setValue('4.30发')
      await amountInput(wrapper).trigger('blur')
      // 失焦不再被静默清空
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe('4.30发')
      expect(hasErrorStatus(wrapper)).toBe(true)
      expect(submitButton(wrapper).attributes('disabled')).toBeDefined()
      // 修正：红态立即解除（可走出的闭环）
      await amountInput(wrapper).setValue('4.30')
      expect(hasErrorStatus(wrapper)).toBe(false)
      expect(submitButton(wrapper).attributes('disabled')).toBeUndefined()
    })

    it('清空后未失焦不红；失焦红；重新输入合法解除（行内模式回填金额）', async () => {
      const wrapper = mount(RefundForm, { props: { fixedTarget: fixedTx } })
      // 行内模式打开即按原交易金额回填（30），合法回填不红
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe('30')
      expect(hasErrorStatus(wrapper)).toBe(false)
      await amountInput(wrapper).setValue('')
      expect(hasErrorStatus(wrapper)).toBe(false)
      await amountInput(wrapper).trigger('blur')
      expect(hasErrorStatus(wrapper)).toBe(true)
      await amountInput(wrapper).setValue('12')
      expect(hasErrorStatus(wrapper)).toBe(false)
    })

    it('保存尝试时空值红显兜底，不发起提交（格式类 toast 被红态取代）', async () => {
      const wrapper = mount(RefundForm)
      await submitButton(wrapper).trigger('click')
      expect(hasErrorStatus(wrapper)).toBe(true)
      expect(createCalls()).toHaveLength(0)
    })

    it('行内模式合法回填（30 元）不显示红态、保存可点', () => {
      const wrapper = mount(RefundForm, { props: { fixedTarget: fixedTx } })
      expect(hasErrorStatus(wrapper)).toBe(false)
      expect(submitButton(wrapper).attributes('disabled')).toBeUndefined()
    })

    it('纯零/负数不红、保存可点、提交走业务类校验通道（不发起 create_transaction）', async () => {
      const wrapper = mount(RefundForm, { props: { fixedTarget: fixedTx } })
      await amountInput(wrapper).setValue('0')
      expect(hasErrorStatus(wrapper)).toBe(false)
      expect(submitButton(wrapper).attributes('disabled')).toBeUndefined()
      await submitButton(wrapper).trigger('click')
      await flushPromises()
      expect(createCalls()).toHaveLength(0)
      // 负数可解析（非格式错误闭集），同走提交通道
      await amountInput(wrapper).setValue('-5')
      expect(hasErrorStatus(wrapper)).toBe(false)
      await submitButton(wrapper).trigger('click')
      await flushPromises()
      expect(createCalls()).toHaveLength(0)
    })

    it('创建成功后表单不留潜伏红态（清空金额但初始为空不红，ADR-0058 决策 2）', async () => {
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'create_transaction') return Promise.resolve('refund-id')
        return Promise.resolve([])
      })
      const wrapper = mount(RefundForm, { props: { fixedTarget: fixedTx } })
      // 行内模式金额预填 30：先清空（未失焦不红），再制造一次保存尝试（空值兜底
      // 红态、静默中止）与失焦（时机标志置位），最后填合法值提交
      await amountInput(wrapper).setValue('')
      await submitButton(wrapper).trigger('click')
      expect(hasErrorStatus(wrapper)).toBe(true)
      expect(createCalls()).toHaveLength(0)
      await amountInput(wrapper).setValue('12')
      await amountInput(wrapper).trigger('blur')
      await submitButton(wrapper).trigger('click')
      await flushPromises()
      expect(createCalls()).toHaveLength(1)
      // 成功后金额清空且时机标志重置：不显红态
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe('')
      expect(hasErrorStatus(wrapper)).toBe(false)
      expect(wrapper.emitted('created')).toHaveLength(1)
    })
  })
})
