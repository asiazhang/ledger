import { describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { makeTransaction } from './factories'
import { mount, flushPromises } from '@vue/test-utils'
import { NSelect } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import TransferForm from '@/components/TransferForm.vue'
import type { Account, Transaction } from '@ledger/types'


const mockAccounts: Account[] = [
  {
    id: 'acc-1', name: '现金', type: 'cash', currency_code: 'CNY',
    initial_balance_cents: 0, created_at: '2026-01-01T00:00:00Z', is_hidden: false,
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
  {
    id: 'acc-2', name: '银行', type: 'bank', currency_code: 'CNY',
    initial_balance_cents: 0, created_at: '2026-01-01T00:00:00Z', is_hidden: false,
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
]

describe('TransferForm.vue', () => {
  beforeEach(async () => {
    // 参考命令桩统一走接缝（issue #725）：币种与规范夹具等值流入，账户保留本文件夹具
    wireInvokeSeam({
      overrides: {
        list_accounts: mockAccounts,
      },
    })
    // Pre-load store so components have data
    const store = useReferenceStore()
    await store.refresh()
  })

  it('挂载并显示提交按钮文本', () => {
    const wrapper = mount(TransferForm)
    expect(wrapper.text()).toContain('记转账')
  })

  // ---- 字段错误态（ADR-0058 / issue #415，行为接缝）：断言用户可见状态——
  // 输入的错误态呈现（n-input--error-status）与保存按钮可用性，不断言内部状态变量。
  // 红态时机路径与支出/收入形态（#414 先例）完全一致。
  /** 金额输入框（placeholder「金额」） */
  function amountInput(wrapper: ReturnType<typeof mount>) {
    return wrapper.find('input[placeholder="金额"]')
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

  /** 经内层 NSelect 注入账户选择（0=币种，1=转出账户，2=转入账户） */
  function selectAccount(wrapper: ReturnType<typeof mount>, index: number, accountId: string) {
    return wrapper.findAllComponents(NSelect)[index].vm.$emit('update:value', accountId)
  }

  function createCalls() {
    return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')
  }

  describe('字段错误态（ADR-0058 / #415）', () => {
    const editingTx: Transaction = {
      id: 'txn-1',
      kind: 'transfer',
      amount_cents: 50000,
      currency_code: 'CNY',
      amount_native_cents: 50000,
      account_id: 'acc-1',
      to_account_id: 'acc-2',
      funding_account_id: null,
      category_id: null,
      merchant_id: null,
      policy_id: null,
      source: null,
      convert: null,
      refund_of_transaction_id: null,
      note: null,
      date: '2026-02-01',
      created_at: '2026-02-01T00:00:00Z',
      updated_at: '2026-02-01T00:00:00Z',
      version: 1,
      device_id: 'test',
      is_deleted: false,
    }

    it('初始为空不红，保存按钮可点', () => {
      const wrapper = mount(TransferForm)
      expect(hasErrorStatus(wrapper)).toBe(false)
      expect(submitButton(wrapper).attributes('disabled')).toBeUndefined()
    })

    it('输入解析失败文本（4.30发）即时红显、保存禁用、非法文本原样保留', async () => {
      const wrapper = mount(TransferForm)
      await amountInput(wrapper).setValue('4.30发')
      expect(hasErrorStatus(wrapper)).toBe(true)
      expect(submitButton(wrapper).attributes('disabled')).toBeDefined()
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe('4.30发')
    })

    it('多个小数点（4.3.0）与超两位小数（4.305）同样即时红显', async () => {
      const wrapper = mount(TransferForm)
      await amountInput(wrapper).setValue('4.3.0')
      expect(hasErrorStatus(wrapper)).toBe(true)
      await amountInput(wrapper).setValue('4.305')
      expect(hasErrorStatus(wrapper)).toBe(true)
    })

    it('非法文本失焦不清空、红态持续；修正后红态解除、保存恢复可点', async () => {
      const wrapper = mount(TransferForm)
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

    it('清空后未失焦不红；失焦红；重新输入合法解除', async () => {
      const wrapper = mount(TransferForm)
      await amountInput(wrapper).setValue('12')
      await amountInput(wrapper).setValue('')
      expect(hasErrorStatus(wrapper)).toBe(false)
      await amountInput(wrapper).trigger('blur')
      expect(hasErrorStatus(wrapper)).toBe(true)
      await amountInput(wrapper).setValue('12')
      expect(hasErrorStatus(wrapper)).toBe(false)
    })

    it('保存尝试时空值红显兜底，不发起提交（格式类 toast 被红态取代）', async () => {
      const wrapper = mount(TransferForm)
      await submitButton(wrapper).trigger('click')
      expect(hasErrorStatus(wrapper)).toBe(true)
      expect(createCalls()).toHaveLength(0)
    })

    it('编辑弹窗合法回填（500 元）不显示红态、保存可点', () => {
      const wrapper = mount(TransferForm, {
        props: { editing: editingTx },
      })
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe('500')
      expect(hasErrorStatus(wrapper)).toBe(false)
      expect(submitButton(wrapper).attributes('disabled')).toBeUndefined()
    })

    it('纯零/负数不红、保存可点、提交走业务类校验通道（不发起 create_transaction）', async () => {
      const wrapper = mount(TransferForm)
      await selectAccount(wrapper, 1, 'acc-1')
      await selectAccount(wrapper, 2, 'acc-2')
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
      // 重桩：提交成功走 create_transaction，参考命令保留本文件夹具（issue #725）
      wireInvokeSeam({
        overrides: {
          list_accounts: mockAccounts,
          create_transaction: 'new-id',
        },
      })
      const wrapper = mount(TransferForm)
      // 先制造一次保存尝试与失焦（时机标志置位），再填合法值提交
      await submitButton(wrapper).trigger('click')
      await amountInput(wrapper).setValue('12')
      await amountInput(wrapper).trigger('blur')
      await selectAccount(wrapper, 1, 'acc-1')
      await selectAccount(wrapper, 2, 'acc-2')
      await submitButton(wrapper).trigger('click')
      await flushPromises()
      expect(createCalls()).toHaveLength(1)
      // 成功后金额清空且时机标志重置：不显红态
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe('')
      expect(hasErrorStatus(wrapper)).toBe(false)
    })
  })

  // 普通转账表单不暴露商户输入位（ADR-0092 决策 4），但编辑时行上商户必须随行
  // 保留——形态退化（借贷账户被换成普通账户）退回普通转账时不静默清数据（决策 5）。
  describe('编辑保留行上商户（issue #875 / ADR-0092）', () => {
    const editingTxWithMerchant: Transaction = makeTransaction({
      id: 'txn-m',
      kind: 'transfer',
      account_id: 'acc-1',
      to_account_id: 'acc-2',
      funding_account_id: null,
      merchant_id: 'mch-1',
      date: '2026-02-01',
    })

    it('表单不渲染商户输入位，但提交把原商户 id 原样带回', async () => {
      wireInvokeSeam({ overrides: { list_accounts: mockAccounts, update_transaction: undefined } })
      const wrapper = mount(TransferForm, { props: { editing: editingTxWithMerchant } })
      expect(wrapper.text()).not.toContain('商户')

      await submitButton(wrapper).trigger('click')
      await flushPromises()

      expect(mockInvoke).toHaveBeenCalledWith('update_transaction', {
        id: 'txn-m',
        input: expect.objectContaining({ kind: 'transfer', merchant_id: 'mch-1' }),
      })
    })
  })
})
