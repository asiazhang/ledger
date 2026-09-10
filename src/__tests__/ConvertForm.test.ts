import { describe, it, expect, beforeEach, beforeAll } from 'vitest'
import { wireInvokeSeam } from './helpers/invoke-mock'
import { findButton } from './helpers/dom'
import { mount } from '@vue/test-utils'
import { useReferenceStore } from '@/stores/reference'
import ConvertForm from '@/components/ConvertForm.vue'
import type { Account, Transaction, TransactionConvert } from '@/types'

// jsdom 不实现 scrollTo：naive-ui 打开虚拟滚动下拉时会调用，提前 polyfill 避免 unhandled rejection
beforeAll(() => {
  Element.prototype.scrollTo = () => {}
})

const mockAccounts: Account[] = [
  {
    id: 'acc-inv',
    name: '基金户',
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

const editingTx: Transaction = {
  id: 'txn-cv-1',
  kind: 'convert',
  amount_cents: 100000,
  currency_code: 'CNY',
  amount_native_cents: 100000,
  account_id: 'acc-inv',
  to_account_id: null,
  funding_account_id: null,
  category_id: null,
  merchant_id: null,
  policy_id: null,
  source: null,
  convert: null,
  refund_of_transaction_id: null,
  note: '换仓',
  date: '2026-02-01',
  created_at: '2026-02-01T01:00:00Z',
  updated_at: '2026-02-01T01:00:00Z',
  version: 1,
  device_id: 'test',
  is_deleted: false,
}

const editingConvert: TransactionConvert = {
  out_instrument_id: 'ins-out',
  out_symbol: '006793',
  out_instrument_name: '转出基金',
  out_quantity: 100.5,
  out_amount_cents: 110550,
  in_instrument_id: 'ins-in',
  in_symbol: '519700',
  in_instrument_name: '转入基金',
  in_quantity: 99.75,
  in_amount_cents: 109725,
  fee_cents: 150,
  carried_cost_cents: 100000,
  currency_code: 'CNY',
}

const OUT_SHARES_PLACEHOLDER = '确认转出份额（以确认单为准）'
const IN_SHARES_PLACEHOLDER = '确认转入份额（以确认单为准）'

/** 份额输入 DOM（按 placeholder 定位，避免与备注 NInput 混淆） */
function sharesInput(wrapper: ReturnType<typeof mount>, placeholder: string) {
  const el = wrapper.get(`input[placeholder="${placeholder}"]`)
  return el
}

/** 输入框所属 NInput 根元素（错误态 class 挂载处） */
function hasErrorStatus(wrapper: ReturnType<typeof mount>, placeholder: string) {
  const el = sharesInput(wrapper, placeholder).element.closest('.n-input')
  expect(el).not.toBeNull()
  return (el as Element).classList.contains('n-input--error-status')
}

beforeEach(async () => {
  wireInvokeSeam({ overrides: { list_accounts: mockAccounts, list_instruments: { items: [], total: 0 } } })
  await useReferenceStore().refresh()
})

describe('ConvertForm.vue（ADR-0099 / issue #979）', () => {
  it('创建形态：渲染两腿标签与份额/金额输入，提交按钮为传入文案，不显示结转成本', () => {
    const wrapper = mount(ConvertForm, { props: { submitLabel: '记转换' } })
    expect(wrapper.text()).toContain('转出标的')
    expect(wrapper.text()).toContain('转入标的')
    expect(wrapper.text()).toContain('转出份额')
    expect(wrapper.text()).toContain('转入份额')
    expect(wrapper.text()).toContain('转出金额')
    expect(wrapper.text()).toContain('转入金额')
    // 两腿单价为只读反算展示
    expect(wrapper.text()).toContain('转出单价（反算）')
    expect(wrapper.text()).toContain('转入单价（反算）')
    // 创建形态无结转成本（行金额锚点尚不存在）
    expect(wrapper.text()).not.toContain('结转成本')
    expect(findButton(wrapper, '记转换')).toBeTruthy()
  })

  it('编辑形态：两腿全量回填、显示结转成本、提交按钮为「保存修改」', () => {
    const wrapper = mount(ConvertForm, {
      props: {
        submitLabel: '记转换',
        editing: editingTx,
        convert: editingConvert,
      },
    })
    expect((sharesInput(wrapper, OUT_SHARES_PLACEHOLDER).element as HTMLInputElement).value).toBe(
      '100.5',
    )
    expect((sharesInput(wrapper, IN_SHARES_PLACEHOLDER).element as HTMLInputElement).value).toBe(
      '99.75',
    )
    expect(wrapper.text()).toContain('结转成本')
    expect(findButton(wrapper, '保存修改')).toBeTruthy()
    // 合法回填不显红态
    expect(hasErrorStatus(wrapper, OUT_SHARES_PLACEHOLDER)).toBe(false)
    expect(hasErrorStatus(wrapper, IN_SHARES_PLACEHOLDER)).toBe(false)
  })

  it('份额解析失败即时红显、保存禁用；修正后红态解除', async () => {
    const wrapper = mount(ConvertForm, { props: { submitLabel: '记转换' } })
    await sharesInput(wrapper, OUT_SHARES_PLACEHOLDER).setValue('4.30发')
    expect(hasErrorStatus(wrapper, OUT_SHARES_PLACEHOLDER)).toBe(true)
    expect(findButton(wrapper, '记转换')!.attributes('disabled')).toBeDefined()
    // 非法文本原样保留（不拦截、不静默丢弃）
    expect((sharesInput(wrapper, OUT_SHARES_PLACEHOLDER).element as HTMLInputElement).value).toBe(
      '4.30发',
    )
    await sharesInput(wrapper, OUT_SHARES_PLACEHOLDER).setValue('100')
    expect(hasErrorStatus(wrapper, OUT_SHARES_PLACEHOLDER)).toBe(false)
    expect(findButton(wrapper, '记转换')!.attributes('disabled')).toBeUndefined()
  })

  it('两侧份额错误态各自独立（转入侧超四位小数即时红，转出侧不受牵连）', async () => {
    const wrapper = mount(ConvertForm, { props: { submitLabel: '记转换' } })
    await sharesInput(wrapper, IN_SHARES_PLACEHOLDER).setValue('1.23456')
    expect(hasErrorStatus(wrapper, IN_SHARES_PLACEHOLDER)).toBe(true)
    expect(hasErrorStatus(wrapper, OUT_SHARES_PLACEHOLDER)).toBe(false)
  })
})
