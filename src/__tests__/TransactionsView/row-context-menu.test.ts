import { merchantDb, makeTxn, mountView, listCalls, lastListFilter, tablePagination, bodyRows, deleteCalls, createCalls, openMenuOnRow, rowMenu, rowMenuKeys, selectRowMenu, setTxnDb, setMerchantDb, pushMock, SHELL_DEFAULTS, SHELL_OVERRIDES } from './common'
import { mockInvoke, wireInvokeSeam, type InvokeSeamDispatcher } from '@ledger/test-support/invoke-mock'
import { clickDialogButton, dialogText, pressReleaseOnDialogMask, visibleModalText } from '@ledger/test-support/dom'
import { describe, it, expect, beforeEach } from 'vitest'
import ConvertDetail from '@/components/ConvertDetail.vue'
import SplitDetail from '@/components/SplitDetail.vue'
import DividendDetail from '@/components/DividendDetail.vue'
import { mount, flushPromises } from '@vue/test-utils'
import { NButton, NDataTable, NPopconfirm, NSelect, NModal, NInput, NInputNumber } from 'naive-ui'
import CategoryForm from '@/components/CategoryForm.vue'
import TransferForm from '@/components/TransferForm.vue'
import InvestmentForm from '@/components/InvestmentForm.vue'
import RefundForm from '@/components/RefundForm.vue'
import AddItemForm from '@/components/AddItemForm.vue'
import MerchantLink from '@/components/MerchantLink.vue'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount, formatQuantity } from '@/utils/money'
import { refCurrencies } from '@ledger/test-support/reference-stubs'
import type { Transaction } from '@ledger/types'

// 金额断言委托形态（issue #770）：期待值调同一 formatAmount 实现，格式规则唯一归属其专测
const cny = refCurrencies[0]

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

  it('交易行「⋯」常显（ADR-0088 决策 6 / issue #843）：操作列末位，每行一枚「⋯」', async () => {
    const wrapper = await mountView()
    const cols = wrapper.findComponent(NDataTable).props('columns') as Array<{ key?: string }>
    expect(cols[cols.length - 1].key).toBe('actions')
    expect(wrapper.findAll('.row-actions-btn')).toHaveLength(3)
    expect(wrapper.findAllComponents(NPopconfirm)).toHaveLength(0)
  })

  it('点「⋯」打开的菜单项集合与右键一致（同一行菜单编排工厂 open 入口）', async () => {
    const wrapper = await mountView()
    // expense 行：右键与「⋯」点开集合一致
    await openMenuOnRow(wrapper, 0)
    expect(rowMenuKeys(wrapper)).toEqual(['edit', 'refund', 'add-item', 'menu-divider', 'delete'])
    await wrapper.findAll('.row-actions-btn')[0].trigger('click')
    await flushPromises()
    expect(rowMenu(wrapper).props('show')).toBe(true)
    expect(rowMenuKeys(wrapper)).toEqual(['edit', 'refund', 'add-item', 'menu-divider', 'delete'])
    // income 行：「⋯」与右键集合一致（编辑 + 删除）
    await openMenuOnRow(wrapper, 1)
    const rightClickKeys = rowMenuKeys(wrapper)
    await wrapper.findAll('.row-actions-btn')[1].trigger('click')
    await flushPromises()
    expect(rowMenuKeys(wrapper)).toEqual(rightClickKeys)
  })

  it('「⋯」选中的动作与右键同一分派：点删除弹二次确认（issue #151）', async () => {
    const wrapper = await mountView()
    await wrapper.findAll('.row-actions-btn')[0].trigger('click')
    await flushPromises()
    await selectRowMenu(wrapper, 'delete')
    expect(dialogText()).toContain('删除后不可恢复')
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
    expect(visibleModalText()).toContain(formatAmount(3000, cny))
    expect(visibleModalText()).toContain('现金')
    // 金额默认原交易金额（可改，字段错误态改造后为自由文本输入框，ADR-0058 / #415），
    // 币种/账户锁定（disabled）
    expect((form.find('input[placeholder="退款金额"]').element as HTMLInputElement).value).toBe('30')
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
    form.find('input[placeholder="退款金额"]').setValue('12')
    await flushPromises()
    // 一次性委托桩（接缝钦定形态）：领域命令自接，其余委托回接缝分发器
    const base = mockInvoke.getMockImplementation()!
    mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'create_transaction' ? Promise.resolve('refund-id') : base(cmd, args))
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
      // 一次性委托桩（接缝钦定形态）：领域命令自接，其余委托回接缝分发器
      const base = mockInvoke.getMockImplementation()!
      mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
        cmd === 'create_transaction' ? Promise.resolve(`refund-${round}`) : base(cmd, args))
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
    // 金额回填（NInput，字段错误态改造后自由文本承载，ADR-0058 / #414；
    // 表单内首个 NInput 即金额）
    expect(form.getComponent(NInput).props('value')).toBe('30')
    expect(form.text()).toContain('保存修改')
    // 回填备注（NInput 的 value，非文本节点；备注是表单末位 NInput）
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
    form.getComponent(NInput).vm.$emit('update:value', '45')
    await flushPromises()
    // 一次性委托桩（接缝钦定形态）：领域命令自接，其余委托回接缝分发器
    const base = mockInvoke.getMockImplementation()!
    mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'update_transaction' ? Promise.resolve() : base(cmd, args))
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
    // 一次性委托桩（接缝钦定形态）：领域命令自接，其余委托回接缝分发器
    const base = mockInvoke.getMockImplementation()!
    mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'update_transaction'
        ? Promise.reject(new Error('账户不存在'))
        : base(cmd, args))
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
    // 一次性委托桩（接缝钦定形态）：领域命令自接，其余委托回接缝分发器
    const base = mockInvoke.getMockImplementation()!
    mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'update_transaction' ? Promise.resolve() : base(cmd, args))
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
    makeTxn(4, 'acc-inv', {
      kind: 'convert',
      amount_cents: 100000,
      note: '换仓',
      date: '2026-02-01',
      source: {
        kind: 'instrument',
        entity_id: 'ins-1',
        display_name: '006793 转出基金',
        status: null,
      },
      convert: {
        to_instrument_id: 'ins-2',
        to_symbol: '519700',
        to_quantity: 99.75,
        out_amount_cents: 110550,
        in_amount_cents: 109725,
      },
    }),
    makeTxn(5, 'acc-1', {
      kind: 'split',
      // 无现金腿：行金额恒 0（ADR-0106 决策 1），列表金额列按空值口径呈现
      amount_cents: 0,
      amount_native_cents: 0,
      note: '年度结转',
      date: '2026-03-01',
      source: {
        kind: 'instrument',
        entity_id: 'ins-1',
        display_name: '502010 证券基金',
        status: null,
      },
    }),
    makeTxn(6, 'acc-1', {
      kind: 'dividend',
      // 有现金腿：行金额 = 分红金额（ADR-0109 / #1078），列表金额列照常展示
      amount_cents: 3000,
      amount_native_cents: 3000,
      note: '年度分红',
      date: '2026-04-01',
      source: {
        kind: 'instrument',
        entity_id: 'ins-1',
        display_name: '502010 证券基金',
        status: null,
      },
    }),
  ]

  /** 转换两腿读投影（`get_transaction_convert`）：编辑回填数据源（ADR-0099 / #979）。 */
  const convertDetail = {
    out_instrument_id: 'ins-1',
    out_symbol: '006793',
    out_instrument_name: '转出基金',
    out_quantity: 100.5,
    out_amount_cents: 110550,
    in_instrument_id: 'ins-2',
    in_symbol: '519700',
    in_instrument_name: '转入基金',
    in_quantity: 99.75,
    in_amount_cents: 109725,
    fee_cents: 150,
    carried_cost_cents: 100000,
    currency_code: 'CNY',
  }

  const buyTrade = {
    instrument_id: 'ins-1',
    symbol: 'NVDA',
    instrument_name: '英伟达',
    instrument_type: 'stock' as const,
    quantity: 100,
    price_cents: 1500000, // 150 元（万分之一元刻度）
    fee_cents: 500,
  }

  /** 份额调整读投影（`get_transaction_split`，ADR-0106 / issue #1052）：只读详情数据源。 */
  const splitDetail = {
    instrument_id: 'ins-1',
    symbol: '502010',
    instrument_name: '证券基金',
    quantity: 339.76,
  }

  /** 接缝分发器：薄壳表展开合并本组特有覆写，重走唯一接缝
   * （持久叠加桩已禁，守门规则 3；issue #750）。 */
  let base: InvokeSeamDispatcher

  beforeEach(() => {
    setTxnDb([...menuDb])
    base = wireInvokeSeam({
      defaults: SHELL_DEFAULTS,
      overrides: {
        ...SHELL_OVERRIDES,
        // sell 行返回无手续费明细，buy 行返回完整明细；convert 行返回两腿明细
        get_transaction_trade: (args) =>
          Promise.resolve(args?.id === 'txn-002' ? { ...buyTrade, fee_cents: null } : buyTrade),
        get_transaction_convert: (args) =>
          args?.id === 'txn-004'
            ? Promise.resolve(convertDetail)
            : Promise.reject(new Error('unexpected invoke: get_transaction_convert')),
        get_transaction_split: (args) =>
          args?.id === 'txn-005'
            ? Promise.resolve(splitDetail)
            : Promise.reject(new Error('unexpected invoke: get_transaction_split')),
      },
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

  it('buy/sell 行右键菜单含「编辑」，refund 行仍仅「删除」，convert 行仅只读「详情」', async () => {
    const wrapper = await mountView()
    await openMenuOnRow(wrapper, 0)
    expect(rowMenuKeys(wrapper)).toEqual(['edit', 'menu-divider', 'delete'])
    await openMenuOnRow(wrapper, 1)
    expect(rowMenuKeys(wrapper)).toEqual(['edit', 'menu-divider', 'delete'])
    await openMenuOnRow(wrapper, 2)
    expect(rowMenuKeys(wrapper)).toEqual(['delete'])
    // convert：无现金腿 kind 在 UI 无编辑/软删入口，只保留只读详情（ADR-0106 决策 10 / #1048）
    await openMenuOnRow(wrapper, 3)
    expect(rowMenuKeys(wrapper)).toEqual(['detail'])
    // split：同属无现金腿 kind，同规只读详情（ADR-0106 决策 10 / #1052）
    await openMenuOnRow(wrapper, 4)
    expect(rowMenuKeys(wrapper)).toEqual(['detail'])
    // dividend：有现金腿但界面同样只读，仅只读详情（ADR-0109 / #1078）
    await openMenuOnRow(wrapper, 5)
    expect(rowMenuKeys(wrapper)).toEqual(['detail'])
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
    // 数量/单价为自由文本输入框（字段错误态，ADR-0058 / #416），金额自动计算与
    // 手续费仍为数字输入（NInputNumber 顺序：金额 disabled 0 / 手续费 1）
    const numbers = form.findAllComponents(NInputNumber)
    expect(numbers[0].props('disabled')).toBe(true)
    expect(numbers[1].props('value')).toBe(5)
    expect((form.find('input[placeholder="数量"]').element as HTMLInputElement).value).toBe('100')
    expect((form.find('input[placeholder="单价"]').element as HTMLInputElement).value).toBe('150')
    expect(form.text()).toContain('保存修改')
  })

  it('buy 行编辑提交：分派 update_transaction（含投资字段），成功关窗并刷新', async () => {
    const wrapper = await mountView()
    await openEditModal(wrapper, 0)
    const form = wrapper.findComponent(InvestmentForm)
    mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'update_transaction' ? Promise.resolve() : base(cmd, args))
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
    expect(numbers[1].props('value')).toBeNull()
  })

  it('convert 行「详情」：先取转换两腿明细（get_transaction_convert），只读弹窗呈现 A → B、两侧份额与金额、手续费与结转成本，无可编辑/提交面', async () => {
    const wrapper = await mountView()
    await openMenuOnRow(wrapper, 3)
    expect(rowMenuKeys(wrapper)).toEqual(['detail'])
    await selectRowMenu(wrapper, 'detail')
    const convertCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'get_transaction_convert')
    expect(convertCalls).toHaveLength(1)
    expect(convertCalls[0][1]).toMatchObject({ id: 'txn-004' })
    const modal = wrapper.findAllComponents(NModal).find((m) => m.props('title') === '交易详情')!
    expect(modal.props('show')).toBe(true)
    const detail = wrapper.findComponent(ConvertDetail)
    expect(detail.exists()).toBe(true)
    expect(detail.props('transaction')).toMatchObject({ id: 'txn-004' })
    expect(detail.props('convert')).toMatchObject({
      out_instrument_id: 'ins-1',
      in_instrument_id: 'ins-2',
    })
    // 两腿标的（A → B）
    expect(detail.text()).toContain('006793')
    expect(detail.text()).toContain('转出基金')
    expect(detail.text()).toContain('519700')
    expect(detail.text()).toContain('转入基金')
    // 两侧份额与金额
    expect(detail.text()).toContain(formatQuantity(100.5))
    expect(detail.text()).toContain(formatQuantity(99.75))
    expect(detail.text()).toContain(formatAmount(110550, cny))
    expect(detail.text()).toContain(formatAmount(109725, cny))
    // 手续费与结转成本（行金额锚点）
    expect(detail.text()).toContain(formatAmount(150, cny))
    expect(detail.text()).toContain(formatAmount(100000, cny))
    expect(detail.text()).toContain('结转成本')
    // 只读形态：无可编辑输入面、无提交/保存按钮（ADR-0106 决策 10 / #1048）
    expect(detail.findAllComponents(NInput)).toHaveLength(0)
    expect(detail.findAllComponents(NInputNumber)).toHaveLength(0)
    expect(detail.findAllComponents(NButton)).toHaveLength(0)
    expect(detail.text()).not.toContain('保存修改')
  })

  it('split 行桌面档：类型标签「份额调整」、无现金腿金额列按空值口径呈现「-」（ADR-0106 / #1052）', async () => {
    const wrapper = await mountView()
    const row = bodyRows(wrapper)[4]
    expect(row.text()).toContain('份额调整')
    expect(row.text()).not.toContain('买入')
    expect(row.text()).not.toContain('卖出')
    const amountEl = row.find('.amount-cell').element as HTMLElement
    expect(amountEl.textContent).toBe('-')
  })

  it('split 行「详情」：先取份额调整明细（get_transaction_split），只读弹窗呈现标的、带符号份额变动、调整日与账户，无可编辑/删除面', async () => {
    const wrapper = await mountView()
    await openMenuOnRow(wrapper, 4)
    expect(rowMenuKeys(wrapper)).toEqual(['detail'])
    await selectRowMenu(wrapper, 'detail')
    const splitCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'get_transaction_split')
    expect(splitCalls).toHaveLength(1)
    expect(splitCalls[0][1]).toMatchObject({ id: 'txn-005' })
    const modal = wrapper.findAllComponents(NModal).find((m) => m.props('title') === '交易详情')!
    expect(modal.props('show')).toBe(true)
    const detail = wrapper.findComponent(SplitDetail)
    expect(detail.exists()).toBe(true)
    expect(detail.props('transaction')).toMatchObject({ id: 'txn-005' })
    expect(detail.props('split')).toMatchObject({ instrument_id: 'ins-1', quantity: 339.76 })
    // 标的、带符号份额变动（显式 +）、调整日、账户
    expect(detail.text()).toContain('502010')
    expect(detail.text()).toContain('证券基金')
    expect(detail.text()).toContain(`+${formatQuantity(339.76)}`)
    expect(detail.text()).toContain('2026-03-01')
    expect(detail.text()).toContain('现金')
    // 只读形态：无可编辑输入面、无提交/保存按钮、无删除入口（ADR-0106 决策 10 / #1052）
    expect(detail.findAllComponents(NInput)).toHaveLength(0)
    expect(detail.findAllComponents(NInputNumber)).toHaveLength(0)
    expect(detail.findAllComponents(NButton)).toHaveLength(0)
    expect(modal.text()).not.toContain('删除')
    expect(modal.text()).not.toContain('保存修改')
  })

  it('dividend 行桌面档：类型标签「分红」、金额列展示分红金额（ADR-0109 / #1078）', async () => {
    const wrapper = await mountView()
    const row = bodyRows(wrapper)[5]
    expect(row.text()).toContain('分红')
    expect(row.text()).not.toContain('份额调整')
    const amountEl = row.find('.amount-cell').element as HTMLElement
    expect(amountEl.textContent).toBe(formatAmount(3000, cny))
  })

  it('dividend 行「详情」：无扩展读取直接开窗，只读弹窗呈现归属标的、金额、账户与日期，无可编辑/删除面', async () => {
    const wrapper = await mountView()
    await openMenuOnRow(wrapper, 5)
    expect(rowMenuKeys(wrapper)).toEqual(['detail'])
    await selectRowMenu(wrapper, 'detail')
    // 分红无扩展读投影：不触发 convert / split 明细命令（明细在列表行内已在场）。
    expect(
      mockInvoke.mock.calls.filter(
        ([cmd]) => cmd === 'get_transaction_convert' || cmd === 'get_transaction_split',
      ),
    ).toHaveLength(0)
    const modal = wrapper.findAllComponents(NModal).find((m) => m.props('title') === '交易详情')!
    expect(modal.props('show')).toBe(true)
    const detail = wrapper.findComponent(DividendDetail)
    expect(detail.exists()).toBe(true)
    expect(detail.props('transaction')).toMatchObject({ id: 'txn-006' })
    // 归属标的（来源列反查的展示名）、金额、账户与日期
    expect(detail.text()).toContain('502010 证券基金')
    expect(detail.text()).toContain(formatAmount(3000, cny))
    expect(detail.text()).toContain('现金')
    expect(detail.text()).toContain('2026-04-01')
    // 只读形态：无可编辑输入面、无提交/保存按钮、无删除入口（ADR-0109 / #1078）
    expect(detail.findAllComponents(NInput)).toHaveLength(0)
    expect(detail.findAllComponents(NInputNumber)).toHaveLength(0)
    expect(detail.findAllComponents(NButton)).toHaveLength(0)
    expect(modal.text()).not.toContain('删除')
    expect(modal.text()).not.toContain('保存修改')
  })

  it('取买卖明细失败：弹窗不打开并提示错误', async () => {
    const wrapper = await mountView()
    // 一次性委托桩（接缝钦定形态）：领域命令自接，其余委托回接缝分发器
    mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'get_transaction_trade'
        ? Promise.reject(new Error('交易不存在或无买卖明细: txn-001'))
        : base(cmd, args))
    await openEditModal(wrapper, 0)
    expect(editModal(wrapper).props('show')).toBe(false)
    expect(wrapper.findComponent(InvestmentForm).exists()).toBe(false)
  })

  it('编辑提交失败：弹窗不关闭、已填内容不丢', async () => {
    const wrapper = await mountView()
    await openEditModal(wrapper, 0)
    const form = wrapper.findComponent(InvestmentForm)
    // 一次性委托桩（接缝钦定形态）：领域命令自接，其余委托回接缝分发器
    mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'update_transaction'
        ? Promise.reject(new Error('该买入交易已有部分卖出，无法修改'))
        : base(cmd, args))
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

  /** 接缝分发器：薄壳表展开合并本组特有覆写，重走唯一接缝
   * （持久叠加桩已禁，守门规则 3；issue #750）。 */
  let base: InvokeSeamDispatcher

  beforeEach(() => {
    setTxnDb([...menuDb])
    itemList = []
    base = wireInvokeSeam({
      defaults: SHELL_DEFAULTS,
      overrides: {
        ...SHELL_OVERRIDES,
        list_items: () => itemList,
        create_item: () => Promise.resolve('item-new'),
      },
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
    // 一次性委托桩（接缝钦定形态）：领域命令自接，其余委托回接缝分发器
    mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'create_item'
        ? Promise.reject(new Error('该购买交易已创建过物品，不能重复创建（溯源唯一）: txn-001'))
        : base(cmd, args))
    const form = wrapper.findComponent(AddItemForm)
    await form.find('button[data-testid="add-item-confirm"]').trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(AddItemForm).emitted('created')).toBeUndefined()
    expect(addItemModal(wrapper).props('show')).toBe(true)
  })
})
