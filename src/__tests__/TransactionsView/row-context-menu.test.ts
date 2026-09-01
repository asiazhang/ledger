import { mockInvoke, merchantDb, mockCurrencies, mockAccounts, makeTxn, applyListFilter, mountView, listCalls, lastListFilter, tablePagination, bodyRows, deleteCalls, createCalls, openMenuOnRow, rowMenu, rowMenuKeys, selectRowMenu, dialogText, visibleModalText, clickDialogButton, pressReleaseOnDialogMask, setTxnDb, setMerchantDb, pushMock } from './common'
import { describe, it, expect, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NDataTable, NPopconfirm, NSelect, NModal, NInput, NInputNumber } from 'naive-ui'
import CategoryForm from '@/components/CategoryForm.vue'
import TransferForm from '@/components/TransferForm.vue'
import InvestmentForm from '@/components/InvestmentForm.vue'
import RefundForm from '@/components/RefundForm.vue'
import AddItemForm from '@/components/AddItemForm.vue'
import MerchantLink from '@/components/MerchantLink.vue'
import { useReferenceStore } from '@/stores/reference'
import type { Transaction } from '@/types'

describe('TransactionsView 行右键菜单（issue #151）', () => {
  // 混合数据集：expense / income / transfer 行并存，供菜单项可见性与删除/退款断言
  const menuDb: Transaction[] = [
    makeTxn(1, 'acc-1', { kind: 'expense', amount_cents: 3000, note: '咖啡' }),
    makeTxn(2, 'acc-1', { kind: 'income', amount_cents: 5000 }),
    makeTxn(3, 'acc-2', { kind: 'transfer', to_account_id: 'acc-1' }),
  ]

  beforeEach(() => {
    setTxnDb([...menuDb])
  })

  it('expense 行右键出现「编辑」「退款」「加入物品」菜单项，非 expense 可编辑行首项「编辑」（issue #178）', async () => {
    const wrapper = await mountView()
    // expense 行：编辑 + 退款 + 加入物品 + 删除
    await openMenuOnRow(wrapper, 0)
    expect(rowMenu(wrapper).props('show')).toBe(true)
    expect(rowMenuKeys(wrapper)).toEqual(['edit', 'refund', 'add-item', 'menu-divider', 'delete'])
    // income 行：编辑 + 删除
    await openMenuOnRow(wrapper, 1)
    expect(rowMenuKeys(wrapper)).toEqual(['edit', 'menu-divider', 'delete'])
    // transfer 行：编辑 + 删除
    await openMenuOnRow(wrapper, 2)
    expect(rowMenuKeys(wrapper)).toEqual(['edit', 'menu-divider', 'delete'])
  })

  it('refund 行右键仅「删除」（编辑本期边界外，issue #178）', async () => {
    setTxnDb([makeTxn(1, 'acc-1', { kind: 'refund', refund_of_transaction_id: 'txn-000' })])
    const wrapper = await mountView()
    await openMenuOnRow(wrapper, 0)
    expect(rowMenuKeys(wrapper)).toEqual(['delete'])
  })

  it('操作列从表格移除', async () => {
    const wrapper = await mountView()
    const cols = wrapper.findComponent(NDataTable).props('columns') as Array<{ title?: string }>
    expect(cols.some((c) => c.title === '操作')).toBe(false)
    expect(wrapper.findAllComponents(NPopconfirm)).toHaveLength(0)
  })

  it('交易列表展示商户列：商户名来自参考数据 merchantMap（issue #189）', async () => {
    setTxnDb([makeTxn(1, 'acc-1', { merchant_id: 'mch-1' })])
    const wrapper = await mountView()
    const cols = wrapper.findComponent(NDataTable).props('columns') as Array<{ title?: string }>
    expect(cols.some((c) => c.title === '商户')).toBe(true)
    expect(bodyRows(wrapper)[0].text()).toContain('京东')
  })

  it('无商户的交易商户列不渲染商户名（issue #189）', async () => {
    setTxnDb([makeTxn(1, 'acc-1')])
    const wrapper = await mountView()
    expect(bodyRows(wrapper)[0].text()).not.toContain('京东')
    expect(wrapper.findAllComponents(MerchantLink).length).toBe(0)
  })

  it('软删商户后历史交易照常显示商户名（后端含软删列表，issue #189/#191）', async () => {
    setTxnDb([makeTxn(1, 'acc-1', { merchant_id: 'mch-1' })])
    const wrapper = await mountView()
    expect(bodyRows(wrapper)[0].text()).toContain('京东')

    // 商户被软删：后端含软删列表返回 is_deleted=true，merchantMap（含软删）仍可解析名称
    setMerchantDb([{ ...merchantDb[0], is_deleted: true }])
    await useReferenceStore().refresh()
    await flushPromises()
    expect(bodyRows(wrapper)[0].text()).toContain('京东')
  })

  it('商户列可点击下钻：跳转 /transactions?merchant=<id>（issue #191）', async () => {
    setTxnDb([makeTxn(1, 'acc-1', { merchant_id: 'mch-1' })])
    const wrapper = await mountView()
    const link = wrapper.findAllComponents(MerchantLink)[0]
    expect(link.exists()).toBe(true)
    expect(link.text()).toBe('京东')
    expect(link.attributes('title')).toBe('查看该商户的交易')
    await link.find('button').trigger('click')
    expect(pushMock).toHaveBeenLastCalledWith({
      name: 'transactions',
      query: { merchant: 'mch-1' },
    })
  })

  it('删除确认框点遮罩不关闭（issue #252）：确认/取消须显式点击', async () => {
    const wrapper = await mountView()
    await openMenuOnRow(wrapper, 0)
    await selectRowMenu(wrapper, 'delete')
    expect(dialogText()).toContain('删除后不可恢复')
    // 遮罩点击不构成关闭意图：确认框保持打开，也不触发删除
    await pressReleaseOnDialogMask()
    expect(dialogText()).toContain('删除后不可恢复')
    expect(deleteCalls()).toHaveLength(0)
    // 显式动作照常工作：取消关闭且不删除
    await clickDialogButton('取消')
    await flushPromises()
    expect(deleteCalls()).toHaveLength(0)
  })

  it('任意行右键「删除」→ 二次确认后才删除；取消不删', async () => {
    const wrapper = await mountView()
    // 取消：不删除
    await openMenuOnRow(wrapper, 0)
    await selectRowMenu(wrapper, 'delete')
    expect(dialogText()).toContain('删除后不可恢复')
    await clickDialogButton('取消')
    await flushPromises()
    expect(deleteCalls()).toHaveLength(0)
    // 确认：删除该行并刷新
    await openMenuOnRow(wrapper, 0)
    await selectRowMenu(wrapper, 'delete')
    await clickDialogButton('删除')
    await flushPromises()
    expect(deleteCalls()).toHaveLength(1)
    expect(deleteCalls()[0][1]).toMatchObject({ id: 'txn-001' })
    expect(wrapper.text()).toContain('共 2 条')
    // 非 expense 行（income）同样可删除
    await openMenuOnRow(wrapper, 0)
    await selectRowMenu(wrapper, 'delete')
    await clickDialogButton('删除')
    await flushPromises()
    expect(deleteCalls()).toHaveLength(2)
  })

  /** 退款弹窗：视图中存在两个 NModal（记一笔 + 退款），按 title 定位。 */
  function refundModal(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NModal)
      .find((m) => m.props('title') === '退款')!
  }

  it('右键退款：无需选择原交易，展示只读信息并锁定账户/币种，金额默认原交易金额', async () => {
    const wrapper = await mountView()
    await openMenuOnRow(wrapper, 0)
    await selectRowMenu(wrapper, 'refund')
    const modal = refundModal(wrapper)
    expect(modal.props('show')).toBe(true)
    // 独立弹窗内嵌 RefundForm（固定模式，无搜索选择下拉）
    const form = wrapper.findComponent(RefundForm)
    expect(form.exists()).toBe(true)
    // 原交易只读信息：日期 / 金额 / 账户名（teleport 到 body，从卡片查文本）
    expect(visibleModalText()).toContain('2026-01-01')
    expect(visibleModalText()).toContain('¥30')
    expect(visibleModalText()).toContain('现金')
    // 金额默认原交易金额（可改），币种/账户锁定（disabled）
    expect(form.getComponent(NInputNumber).props('value')).toBe(30)
    const lockedSelects = form.findAllComponents(NSelect)
    expect(lockedSelects.length).toBe(2) // 币种 + 账户
    expect(lockedSelects.every((s) => s.props('disabled'))).toBe(true)
  })

  it('右键退款提交：走 kind=refund 写路径并关联原交易，弹窗关闭回到第 1 页', async () => {
    const wrapper = await mountView()
    await openMenuOnRow(wrapper, 0)
    await selectRowMenu(wrapper, 'refund')
    const form = wrapper.findComponent(RefundForm)
    // 修改退款金额为部分退款 ¥12.00
    form.getComponent(NInputNumber).vm.$emit('update:value', 12)
    await flushPromises()
    mockInvoke.mockImplementationOnce((cmd: string) => {
      if (cmd === 'create_transaction') return Promise.resolve('refund-id')
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await form.findAll('button').find((b) => b.text().includes('记退款'))!.trigger('click')
    await flushPromises()
    // 载荷：kind=refund + 关联原交易；账户/币种由后端继承原支出（固定模式展示值）
    expect(createCalls()).toHaveLength(1)
    const [, args] = createCalls()[0] as [string, { input: Record<string, unknown> }]
    expect(args.input).toMatchObject({
      kind: 'refund',
      amount_cents: 1200,
      refund_of_transaction_id: 'txn-001',
      currency_code: 'CNY',
      account_id: 'acc-1',
    })
    // 弹窗关闭、回到第 1 页刷新
    expect(refundModal(wrapper).props('show')).toBe(false)
    expect(lastListFilter()).toMatchObject({ page: 1 })
  })

  it('同一 expense 可再次右键发起退款（部分退款语义，不阻断）', async () => {
    const wrapper = await mountView()
    for (let round = 0; round < 2; round++) {
      await openMenuOnRow(wrapper, 0)
      await selectRowMenu(wrapper, 'refund')
      const form = wrapper.findComponent(RefundForm)
      expect(form.exists()).toBe(true)
      mockInvoke.mockImplementationOnce((cmd: string) => {
        if (cmd === 'create_transaction') return Promise.resolve(`refund-${round}`)
        return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
      })
      await form.findAll('button').find((b) => b.text().includes('记退款'))!.trigger('click')
      await flushPromises()
      expect(refundModal(wrapper).props('show')).toBe(false)
    }
    expect(createCalls()).toHaveLength(2)
    for (const [, args] of createCalls() as Array<[string, { input: Record<string, unknown> }]>) {
      expect(args.input).toMatchObject({ kind: 'refund', refund_of_transaction_id: 'txn-001' })
    }
  })
})

describe('TransactionsView 行右键「编辑」（issue #178）', () => {
  // 混合数据集：expense / income / transfer 行并存，回填与提交分派断言
  const menuDb: Transaction[] = [
    makeTxn(1, 'acc-1', { kind: 'expense', amount_cents: 3000, note: '咖啡', date: '2026-01-05' }),
    makeTxn(2, 'acc-1', { kind: 'income', amount_cents: 5000, note: '工资', date: '2026-01-06' }),
    makeTxn(3, 'acc-2', { kind: 'transfer', amount_cents: 8800, to_account_id: 'acc-1', note: '转账备注' }),
  ]

  beforeEach(() => {
    setTxnDb([...menuDb])
  })

  function updateCalls() {
    return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'update_transaction')
  }

  /** 编辑弹窗：按 title 定位。 */
  function editModal(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAllComponents(NModal).find((m) => m.props('title') === '编辑交易')!
  }

  /** 右键指定行并选「编辑」。 */
  async function openEditModal(wrapper: ReturnType<typeof mount>, index = 0) {
    await openMenuOnRow(wrapper, index)
    await selectRowMenu(wrapper, 'edit')
  }

  it('右键编辑：弹窗打开，表单回填全部业务字段，按钮文案为「保存修改」', async () => {
    const wrapper = await mountView()
    await openEditModal(wrapper, 0)
    expect(editModal(wrapper).props('show')).toBe(true)
    // expense 行分派到分类记账表单（kind 锁死，无类型切换）
    const form = wrapper.findComponent(CategoryForm)
    expect(form.props('kind')).toBe('expense')
    expect(form.props('editing')).toMatchObject({ id: 'txn-001' })
    expect(form.getComponent(NInputNumber).props('value')).toBe(30)
    expect(form.text()).toContain('保存修改')
    // 回填备注（NInput 的 value，非文本节点；NInputNumber 内部也含 NInput，取最后一个）
    const inputs = form.findAllComponents(NInput)
    expect(inputs[inputs.length - 1].props('value')).toBe('咖啡')
    // 编辑弹窗内无另一个分类表单（kind 锁死不可切换）
    expect(wrapper.findAllComponents(CategoryForm)).toHaveLength(1)
    expect(wrapper.findAllComponents(TransferForm)).toHaveLength(0)
  })

  it.each([
    ['income', CategoryForm, 'txn-002'],
    ['transfer', TransferForm, 'txn-003'],
  ] as const)('%s 行编辑：按 kind 分派到对应表单', async (kind, formComponent, expectedId) => {
    const wrapper = await mountView()
    await openEditModal(wrapper, kind === 'income' ? 1 : 2)
    const form = wrapper.findComponent(formComponent)
    expect(form.exists()).toBe(true)
    expect(form.props('editing')).toMatchObject({ id: expectedId })
  })

  it('编辑提交：走 update_transaction（id + 全字段载荷），弹窗关闭且刷新保持当前页', async () => {
    const wrapper = await mountView()
    await openEditModal(wrapper, 0)
    const form = wrapper.findComponent(CategoryForm)
    form.getComponent(NInputNumber).vm.$emit('update:value', 45)
    await flushPromises()
    mockInvoke.mockImplementationOnce((cmd: string) => {
      if (cmd === 'update_transaction') return Promise.resolve()
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await form.findAll('button').find((b) => b.text().includes('保存修改'))!.trigger('click')
    await flushPromises()
    expect(updateCalls()).toHaveLength(1)
    const [, { id, input }] = updateCalls()[0] as unknown as [
      string,
      { id: string; input: Record<string, unknown> },
    ]
    expect(id).toBe('txn-001')
    expect(input).toMatchObject({
      kind: 'expense',
      amount_cents: 4500,
      currency_code: 'CNY',
      account_id: 'acc-1',
      note: '咖啡',
      date: '2026-01-05',
    })
    // 幂等键不可编辑：载荷不含 idempotency_key
    expect(input.idempotency_key).toBeUndefined()
    // 弹窗关闭、列表刷新且保持当前页（不重置到第 1 页）
    expect(editModal(wrapper).props('show')).toBe(false)
    // 列表共 3 条单页，此处仅验证刷新发生（list_transactions 再次调用）
    expect(listCalls().length).toBeGreaterThanOrEqual(2)
  })

  it('编辑提交失败：弹窗不关闭（明确错误由表单提示）', async () => {
    const wrapper = await mountView()
    await openEditModal(wrapper, 0)
    const form = wrapper.findComponent(CategoryForm)
    mockInvoke.mockImplementationOnce((cmd: string) => {
      if (cmd === 'update_transaction') return Promise.reject(new Error('账户不存在'))
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await form.findAll('button').find((b) => b.text().includes('保存修改'))!.trigger('click')
    await flushPromises()
    expect(editModal(wrapper).props('show')).toBe(true)
    expect(updateCalls()).toHaveLength(1)
  })

  it('编辑提交成功后刷新保持当前页与筛选（不重置回第 1 页）', async () => {
    setTxnDb(Array.from({ length: 45 }, (_, i) =>
      makeTxn(i + 1, i % 2 === 0 ? 'acc-2' : 'acc-1'),
    ))
    const wrapper = await mountView()
    // 翻到第 2 页再编辑
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    await openEditModal(wrapper, 0)
    const form = wrapper.findComponent(CategoryForm)
    mockInvoke.mockImplementationOnce((cmd: string) => {
      if (cmd === 'update_transaction') return Promise.resolve()
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await form.findAll('button').find((b) => b.text().includes('保存修改'))!.trigger('click')
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2 })
  })
})

describe('TransactionsView 行右键「编辑」buy/sell（issue #180）', () => {
  const menuDb: Transaction[] = [
    makeTxn(1, 'acc-inv', { kind: 'buy', amount_cents: 15500, note: '建仓买入', date: '2026-01-10' }),
    makeTxn(2, 'acc-inv', { kind: 'sell', amount_cents: 9500, note: '减仓', date: '2026-01-20' }),
    makeTxn(3, 'acc-1', { kind: 'refund', refund_of_transaction_id: 'txn-000' }),
  ]

  const buyTrade = {
    instrument_id: 'ins-1',
    symbol: 'NVDA',
    instrument_name: '英伟达',
    instrument_type: 'stock' as const,
    quantity: 100,
    price_cents: 1500000, // 150 元（万分之一元刻度）
    fee_cents: 500,
  }

  beforeEach(() => {
    setTxnDb([...menuDb])
    mockInvoke.mockImplementation((cmd: string, args?: {
      filter?: Record<string, unknown>
      id?: string
    }) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve(merchantDb)
      if (cmd === 'list_policies') return Promise.resolve([])
      if (cmd === 'list_transactions') {
        const filter = (args?.filter ?? {}) as Record<string, unknown>
        const scoped = applyListFilter(filter)
        const pageSize = (filter.page_size as number) ?? scoped.length
        const page = (filter.page as number) ?? 1
        const start = (page - 1) * pageSize
        return Promise.resolve({
          items: scoped.slice(start, start + pageSize),
          total: scoped.length,
        })
      }
      if (cmd === 'list_items') return Promise.resolve([])
      if (cmd === 'get_transaction_trade') {
        // sell 行返回无手续费明细，buy 行返回完整明细
        return Promise.resolve(args?.id === 'txn-002' ? { ...buyTrade, fee_cents: null } : buyTrade)
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
  })

  function updateCalls() {
    return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'update_transaction')
  }

  function editModal(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAllComponents(NModal).find((m) => m.props('title') === '编辑交易')!
  }

  async function openEditModal(wrapper: ReturnType<typeof mount>, index = 0) {
    await openMenuOnRow(wrapper, index)
    await selectRowMenu(wrapper, 'edit')
  }

  it('buy/sell 行右键菜单含「编辑」，refund 行仍仅「删除」', async () => {
    const wrapper = await mountView()
    await openMenuOnRow(wrapper, 0)
    expect(rowMenuKeys(wrapper)).toEqual(['edit', 'menu-divider', 'delete'])
    await openMenuOnRow(wrapper, 1)
    expect(rowMenuKeys(wrapper)).toEqual(['edit', 'menu-divider', 'delete'])
    await openMenuOnRow(wrapper, 2)
    expect(rowMenuKeys(wrapper)).toEqual(['delete'])
  })

  it('buy 行编辑：先取买卖明细（get_transaction_trade），投资表单回填标的/数量/价格/费用，按钮「保存修改」', async () => {
    const wrapper = await mountView()
    await openEditModal(wrapper, 0)
    const tradeCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'get_transaction_trade')
    expect(tradeCalls).toHaveLength(1)
    expect(tradeCalls[0][1]).toMatchObject({ id: 'txn-001' })
    const form = wrapper.findComponent(InvestmentForm)
    expect(form.exists()).toBe(true)
    expect(form.props('kind')).toBe('buy')
    expect(form.props('editing')).toMatchObject({ id: 'txn-001' })
    expect(form.props('trade')).toMatchObject({ instrument_id: 'ins-1' })
    // NInputNumber 顺序：金额（disabled，0）/ 数量（1）/ 单价（2）/ 手续费（3）
    const numbers = form.findAllComponents(NInputNumber)
    expect(numbers[0].props('disabled')).toBe(true)
    expect(numbers[1].props('value')).toBe(100)
    expect(numbers[2].props('value')).toBe(150)
    expect(numbers[3].props('value')).toBe(5)
    expect(form.text()).toContain('保存修改')
  })

  it('buy 行编辑提交：分派 update_transaction（含投资字段），成功关窗并刷新', async () => {
    const wrapper = await mountView()
    await openEditModal(wrapper, 0)
    const form = wrapper.findComponent(InvestmentForm)
    mockInvoke.mockImplementationOnce((cmd: string) => {
      if (cmd === 'update_transaction') return Promise.resolve()
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await form.findAll('button').find((b) => b.text().includes('保存修改'))!.trigger('click')
    await flushPromises()
    expect(updateCalls()).toHaveLength(1)
    const [, { id, input }] = updateCalls()[0] as unknown as [
      string,
      { id: string; input: Record<string, unknown> },
    ]
    expect(id).toBe('txn-001')
    expect(input).toMatchObject({
      kind: 'buy',
      amount_cents: 0,
      account_id: 'acc-inv',
      note: '建仓买入',
      date: '2026-01-10',
      instrument_id: 'ins-1',
      quantity: 100,
      price_cents: 1500000, // 150 元（万分之一元刻度）
      fee_cents: 500,
    })
    expect(input.idempotency_key).toBeUndefined()
    expect(editModal(wrapper).props('show')).toBe(false)
    expect(listCalls().length).toBeGreaterThanOrEqual(2)
  })

  it('sell 行编辑：明细 fee_cents 为 null 时费用回填为空', async () => {
    const wrapper = await mountView()
    await openEditModal(wrapper, 1)
    const form = wrapper.findComponent(InvestmentForm)
    expect(form.props('kind')).toBe('sell')
    expect(form.props('trade')).toMatchObject({ instrument_id: 'ins-1', fee_cents: null })
    const numbers = form.findAllComponents(NInputNumber)
    expect(numbers[3].props('value')).toBeNull()
  })

  it('取买卖明细失败：弹窗不打开并提示错误', async () => {
    const wrapper = await mountView()
    mockInvoke.mockImplementationOnce((cmd: string) => {
      if (cmd === 'get_transaction_trade') return Promise.reject(new Error('交易不存在或无买卖明细: txn-001'))
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await openEditModal(wrapper, 0)
    expect(editModal(wrapper).props('show')).toBe(false)
    expect(wrapper.findComponent(InvestmentForm).exists()).toBe(false)
  })

  it('编辑提交失败：弹窗不关闭、已填内容不丢', async () => {
    const wrapper = await mountView()
    await openEditModal(wrapper, 0)
    const form = wrapper.findComponent(InvestmentForm)
    mockInvoke.mockImplementationOnce((cmd: string) => {
      if (cmd === 'update_transaction') return Promise.reject(new Error('该买入交易已有部分卖出，无法修改'))
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await form.findAll('button').find((b) => b.text().includes('保存修改'))!.trigger('click')
    await flushPromises()
    expect(editModal(wrapper).props('show')).toBe(true)
    expect(form.props('trade')).toMatchObject({ instrument_id: 'ins-1' })
  })
})

describe('TransactionsView 行右键「加入物品」（issue #119）', () => {
  const menuDb: Transaction[] = [
    makeTxn(1, 'acc-1', { kind: 'expense', amount_cents: 3000, note: '咖啡' }),
    makeTxn(2, 'acc-1', { kind: 'income', amount_cents: 5000 }),
  ]

  /** 已建物品列表（默认空；置灰用例改写为关联 txn-001）。 */
  let itemList: unknown[] = []

  beforeEach(() => {
    setTxnDb([...menuDb])
    itemList = []
    mockInvoke.mockImplementation((cmd: string, args?: {
      filter?: Record<string, unknown>
      id?: string
    }) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve(merchantDb)
      if (cmd === 'list_policies') return Promise.resolve([])
      if (cmd === 'list_transactions') {
        const filter = (args?.filter ?? {}) as Record<string, unknown>
        const scoped = applyListFilter(filter)
        const pageSize = (filter.page_size as number) ?? scoped.length
        const page = (filter.page as number) ?? 1
        const start = (page - 1) * pageSize
        return Promise.resolve({
          items: scoped.slice(start, start + pageSize),
          total: scoped.length,
        })
      }
      if (cmd === 'list_items') return Promise.resolve(itemList)
      if (cmd === 'create_item') return Promise.resolve('item-new')
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
  })

  /** 右键 expense 行并选「加入物品」。 */
  async function openAddItemModal(wrapper: ReturnType<typeof mount>) {
    await openMenuOnRow(wrapper, 0)
    await selectRowMenu(wrapper, 'add-item')
  }

  function addItemModal(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NModal)
      .find((m) => m.props('title') === '加入物品')!
  }

  it('expense 行未建物品：加入物品菜单项可用，选中后弹出确认弹窗', async () => {
    const wrapper = await mountView()
    await openMenuOnRow(wrapper, 0)
    const options = rowMenu(wrapper).props('options') as Array<{
      key?: string
      disabled?: boolean
    }>
    expect(options.find((o) => o.key === 'add-item')).toMatchObject({ disabled: false })
    await selectRowMenu(wrapper, 'add-item')
    expect(addItemModal(wrapper).props('show')).toBe(true)
    const form = wrapper.findComponent(AddItemForm)
    expect(form.exists()).toBe(true)
    expect(form.props('transaction')).toMatchObject({ id: 'txn-001' })
    // income 行无「加入物品」项（但有编辑项，issue #178）
    await openMenuOnRow(wrapper, 1)
    expect(rowMenuKeys(wrapper)).toEqual(['edit', 'menu-divider', 'delete'])
  })

  it('该交易已建物品（溯源指针比对）：加入物品菜单项置灰', async () => {
    itemList = [
      { id: 'item-1', purchase_transaction_id: 'txn-001' },
      { id: 'item-2', purchase_transaction_id: null },
    ]
    const wrapper = await mountView()
    await openMenuOnRow(wrapper, 0)
    const options = rowMenu(wrapper).props('options') as Array<{
      key?: string
      disabled?: boolean
    }>
    expect(options.find((o) => o.key === 'add-item')).toMatchObject({ disabled: true })
  })

  it('确认创建：create_item 携带溯源必填入参，弹窗关闭；物品列表经 ledger:changed 自动重拉', async () => {
    const wrapper = await mountView()
    await openAddItemModal(wrapper)
    const form = wrapper.findComponent(AddItemForm)
    // 名称默认取交易备注，可微调
    form.find('input[placeholder="默认取交易备注，可微调"]').setValue('手冲壶')
    await form.find('button[data-testid="add-item-confirm"]').trigger('click')
    await flushPromises()
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_item')
    expect(calls).toHaveLength(1)
    const [, args] = calls[0] as [string, { input: Record<string, unknown> }]
    expect(args.input).toEqual({
      name: '手冲壶',
      purchase_date: '2026-01-01',
      total_cost_cents: 3000,
      currency_code: 'CNY',
      note: null,
      purchase_transaction_id: 'txn-001',
    })
    expect(addItemModal(wrapper).props('show')).toBe(false)
  })

  it('后端校验失败（重复创建）：弹窗保持打开，错误后不 emit created', async () => {
    const wrapper = await mountView()
    await openAddItemModal(wrapper)
    mockInvoke.mockImplementationOnce((cmd: string) => {
      if (cmd === 'create_item')
        return Promise.reject(new Error('该购买交易已创建过物品，不能重复创建（溯源唯一）: txn-001'))
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const form = wrapper.findComponent(AddItemForm)
    await form.find('button[data-testid="add-item-confirm"]').trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(AddItemForm).emitted('created')).toBeUndefined()
    expect(addItemModal(wrapper).props('show')).toBe(true)
  })
})

