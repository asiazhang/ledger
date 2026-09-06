import { mockInvoke, mountView, setAccountDb, setTxnDb, makeTxn, bodyRows, openMenuOnRow, selectRowMenu } from './common'
import { describe, it, expect, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NButton, NModal, NSelect } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import LendingForm from '@/components/LendingForm.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import TransferForm from '@/components/TransferForm.vue'
import TransactionForm from '@/components/TransactionForm.vue'
import type { Account, Transaction } from '@/types'

/** 覆盖资金侧 + 借出侧 + 负债侧的账户集（经 setAccountDb 注入，issue #374）。 */
const lendingAccounts: Account[] = [
  { id: 'acc-cash', name: '现金', type: 'cash' },
  { id: 'acc-bank', name: '银行', type: 'bank' },
  { id: 'acc-recv-zhang', name: '借出·张三', type: 'receivable' },
  { id: 'acc-debt-li', name: '借入·李四', type: 'debt' },
].map((a) => ({
  currency_code: 'CNY',
  initial_balance_cents: 0,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  version: 1,
  device_id: 'test',
  is_deleted: false,
  is_hidden: false,
  ...a,
})) as Account[]

beforeEach(async () => {
  // common 的 beforeEach（先注册先执行）已建好 pinia 与 mockInvoke；
  // 这里换入借贷账户集并重拉参考数据，供下拉过滤与标签派生消费。
  setAccountDb(lendingAccounts)
  await useReferenceStore().refresh()
})

describe('交易列表借贷类型标签（issue #374：transfer 派生视角，不新增 kind）', () => {
  beforeEach(() => {
    setTxnDb([
      // 借贷四方向：识别唯一依据是两端账户类型
      makeTxn(1, 'acc-cash', { kind: 'transfer', to_account_id: 'acc-recv-zhang', note: '借给张三' }),
      makeTxn(2, 'acc-recv-zhang', { kind: 'transfer', to_account_id: 'acc-cash', note: '张三还钱' }),
      makeTxn(3, 'acc-debt-li', { kind: 'transfer', to_account_id: 'acc-cash', note: '向李四借入' }),
      makeTxn(4, 'acc-cash', { kind: 'transfer', to_account_id: 'acc-debt-li', note: '还李四' }),
      // 普通转账与非转账不受影响
      makeTxn(5, 'acc-cash', { kind: 'transfer', to_account_id: 'acc-bank', note: '普通转账' }),
      makeTxn(6, 'acc-cash', { kind: 'expense', note: '午饭' }),
    ])
  })

  it('借贷转账显示借出/收回/借入/还款专属文案，普通转账仍显示转账、支出不变', async () => {
    const wrapper = await mountView()
    const kindTexts = bodyRows(wrapper).map((r) => r.findAll('.n-data-table-td')[1].text())
    expect(kindTexts).toEqual(['借出', '收回', '借入', '还款', '转账', '支出'])
  })
})

describe('交易列表借贷编辑形态识别（issue #374）', () => {
  beforeEach(() => {
    setTxnDb([
      makeTxn(1, 'acc-cash', { kind: 'transfer', to_account_id: 'acc-recv-zhang', note: '借给张三' }),
      makeTxn(2, 'acc-cash', { kind: 'transfer', to_account_id: 'acc-bank', note: '普通转账' }),
    ])
  })

  function editModal(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAllComponents(NModal).find((m) => m.props('title') === '编辑交易')!
  }

  async function openEditModal(wrapper: ReturnType<typeof mount>, index = 0) {
    await openMenuOnRow(wrapper, index)
    await selectRowMenu(wrapper, 'edit')
  }

  it('借贷转账编辑：以借贷变体回填（LendingForm），普通转账仍走转账表单', async () => {
    const wrapper = await mountView()
    await openEditModal(wrapper, 0)
    expect(editModal(wrapper).props('show')).toBe(true)
    const form = wrapper.findComponent(TransactionForm)
    // 借贷形态：LendingForm 呈现（方向由账户类型派生），无转账表单
    const lending = form.findComponent(LendingForm)
    expect(lending.exists()).toBe(true)
    expect(form.findComponent(TransferForm).exists()).toBe(false)
    // 方向按钮组四方向齐备，当前方向「借出」高亮（primary）
    const dirButtons = lending.findAllComponents(NButton).filter((b) =>
      ['借出', '收回', '借入', '还款'].includes(b.text()),
    )
    expect(dirButtons.map((b) => b.text())).toEqual(['借出', '收回', '借入', '还款'])
    const active = dirButtons.find((b) => b.props('type') === 'primary')
    expect(active?.text()).toBe('借出')
    // 双账户按既有交易回填（转出=现金，转入=借出·张三）；金额同步回填（1 元，字段
    // 错误态改造后金额为自由文本输入框，ADR-0058 / #415）；
    // PinyinSelect 经 attrs 透传（无声明 props），从内层 NSelect 读取声明 prop
    const selects = lending.findAllComponents(PinyinSelect)
    const [fromSelect, toSelect] = selects.map((s) => s.findComponent(NSelect))
    expect(fromSelect.props('value')).toBe('acc-cash')
    expect(toSelect.props('value')).toBe('acc-recv-zhang')
    expect((lending.find('input[placeholder="金额"]').element as HTMLInputElement).value).toBe('1')
  })

  it('普通转账编辑仍分派 TransferForm（不误判为借贷）', async () => {
    const wrapper = await mountView()
    await openEditModal(wrapper, 1)
    const form = wrapper.findComponent(TransactionForm)
    expect(form.findComponent(TransferForm).exists()).toBe(true)
    expect(form.findComponent(LendingForm).exists()).toBe(false)
  })
})

describe('记一笔借贷入口完整链路（issue #374）', () => {
  // jsdom 的 document.body 跨测试共享：清掉前序测试遗留的 teleport 内容
  beforeEach(() => {
    document.body.innerHTML = ''
  })

  async function openLendModal(wrapper: ReturnType<typeof mount>, label: string) {
    const arrow = wrapper.find('button[aria-label="更多记账类型"]')
    await arrow.trigger('click')
    await flushPromises()
    const item = [...document.body.querySelectorAll('.n-dropdown-option')].find(
      (el) => el.textContent?.trim() === label,
    )
    expect(item, `下拉菜单中应存在「${label}」项`).toBeDefined()
    ;(item!.querySelector('.n-dropdown-option-body') as HTMLElement).click()
    await flushPromises()
  }

  it('「借出」入口：账户下拉按方向过滤（转出=资金账户/转入=借出款），提交落 transfer 与转账同构', async () => {
    const wrapper = await mountView()
    await openLendModal(wrapper, '借出')
    expect(wrapper.findComponent(NModal).props('title')).toBe('记一笔 · 借出')
    const form = wrapper.findComponent(TransactionForm).findComponent(LendingForm)

    // 账户过滤：转出侧只有资金账户，转入侧只有 receivable 账户（经内层 NSelect 读声明 prop）
    const selects = form.findAllComponents(PinyinSelect)
    const [fromSelect, toSelect] = selects.map((s) => s.findComponent(NSelect))
    const fromOptions = fromSelect.props('options') as Array<{ value: string }>
    const toOptions = toSelect.props('options') as Array<{ value: string }>
    expect(fromOptions.map((o) => o.value)).toEqual(['acc-cash', 'acc-bank'])
    expect(toOptions.map((o) => o.value)).toEqual(['acc-recv-zhang'])

    // 填表提交
    fromSelect.vm.$emit('update:value', 'acc-cash')
    toSelect.vm.$emit('update:value', 'acc-recv-zhang')
    form.find('input[placeholder="金额"]').setValue('1000')
    await flushPromises()
    // 参考命令兜底走共享助手（issue #725）：领域命令自接，其余委托回基础桩
    const base = mockInvoke.getMockImplementation()!
    mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'create_transaction' ? Promise.resolve('new-id') : base(cmd, args))
    const submitBtn = form.findAllComponents(NButton).find((b) => b.text() === '记借出')!
    await submitBtn.trigger('click')
    await flushPromises()

    // 写入命令与转账相同：kind=transfer + 方向即双账户填法
    const createCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')
    expect(createCalls).toHaveLength(1)
    const [, args] = createCalls[0] as [string, { input: Record<string, unknown> }]
    expect(args.input).toMatchObject({
      kind: 'transfer',
      amount_cents: 100000,
      currency_code: 'CNY',
      account_id: 'acc-cash',
      to_account_id: 'acc-recv-zhang',
    })
  })

  it('「借入」入口预置反向过滤（转出=负债/转入=资金账户）', async () => {
    const wrapper = await mountView()
    await openLendModal(wrapper, '借入')
    expect(wrapper.findComponent(NModal).props('title')).toBe('记一笔 · 借入')
    const form = wrapper.findComponent(TransactionForm).findComponent(LendingForm)
    const selects = form.findAllComponents(PinyinSelect)
    const [fromSelect, toSelect] = selects.map((s) => s.findComponent(NSelect))
    const fromOptions = fromSelect.props('options') as Array<{ value: string }>
    const toOptions = toSelect.props('options') as Array<{ value: string }>
    expect(fromOptions.map((o) => o.value)).toEqual(['acc-debt-li'])
    expect(toOptions.map((o) => o.value)).toEqual(['acc-cash', 'acc-bank'])
  })
})

describe('借贷金额字段错误态（ADR-0058 / issue #416，共享接缝装配验证）', () => {
  // jsdom 的 document.body 跨测试共享：清掉前序测试遗留的 teleport 内容
  beforeEach(() => {
    document.body.innerHTML = ''
  })

  async function openLendModal(wrapper: ReturnType<typeof mount>, label: string) {
    const arrow = wrapper.find('button[aria-label="更多记账类型"]')
    await arrow.trigger('click')
    await flushPromises()
    const item = [...document.body.querySelectorAll('.n-dropdown-option')].find(
      (el) => el.textContent?.trim() === label,
    )
    expect(item, `下拉菜单中应存在「${label}」项`).toBeDefined()
    ;(item!.querySelector('.n-dropdown-option-body') as HTMLElement).click()
    await flushPromises()
  }

  function amountInput(form: ReturnType<typeof mount>) {
    return form.find('input[placeholder="金额"]')
  }

  function hasErrorStatus(form: ReturnType<typeof mount>) {
    const el = amountInput(form).element.closest('.n-input')
    expect(el).not.toBeNull()
    return (el as Element).classList.contains('n-input--error-status')
  }

  function submitButton(form: ReturnType<typeof mount>, label: string) {
    return form.findAllComponents(NButton).find((b) => b.text() === label)!
  }

  it('金额输入解析失败文本（4.30发）即时红显、记借出禁用、非法文本原样保留', async () => {
    const wrapper = await mountView()
    await openLendModal(wrapper, '借出')
    const form = wrapper.findComponent(TransactionForm).findComponent(LendingForm)
    await amountInput(form).setValue('4.30发')
    expect(hasErrorStatus(form)).toBe(true)
    expect(submitButton(form, '记借出').attributes('disabled')).toBeDefined()
    // 非法文本原样保留（不拦截不静默丢弃）
    expect((amountInput(form).element as HTMLInputElement).value).toBe('4.30发')
  })

  it('清空失焦红、修正解除、记借出恢复可点', async () => {
    const wrapper = await mountView()
    await openLendModal(wrapper, '借出')
    const form = wrapper.findComponent(TransactionForm).findComponent(LendingForm)
    await amountInput(form).setValue('100')
    await amountInput(form).setValue('')
    expect(hasErrorStatus(form)).toBe(false)
    await amountInput(form).trigger('blur')
    expect(hasErrorStatus(form)).toBe(true)
    await amountInput(form).setValue('100')
    expect(hasErrorStatus(form)).toBe(false)
    expect(submitButton(form, '记借出').attributes('disabled')).toBeUndefined()
  })

  it('保存尝试空值红显兜底，不发起提交（红态取代格式类 toast）', async () => {
    const wrapper = await mountView()
    await openLendModal(wrapper, '借出')
    const form = wrapper.findComponent(TransactionForm).findComponent(LendingForm)
    await submitButton(form, '记借出').trigger('click')
    await flushPromises()
    expect(hasErrorStatus(form)).toBe(true)
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')).toHaveLength(0)
  })
})
