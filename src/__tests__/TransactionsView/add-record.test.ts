import { mockInvoke, mountView, listCalls, lastListFilter, tablePagination, createCalls } from './common'
import { describe, it, expect, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NSelect, NButton, NModal, NInputNumber, NRadioGroup } from 'naive-ui'
import CategoryForm from '@/components/CategoryForm.vue'
import TransferForm from '@/components/TransferForm.vue'
import InvestmentForm from '@/components/InvestmentForm.vue'
import TransactionForm from '@/components/TransactionForm.vue'

describe('TransactionsView 记一笔 Modal（issue #141）', () => {
  /** 打开「记一笔」弹窗（点击工具栏按钮后等弹窗挂载）。 */
  async function openCreateModal(wrapper: ReturnType<typeof mount>) {
    const btn = wrapper.findAll('button').find((b) => b.text().includes('记一笔'))!
    await btn.trigger('click')
    await flushPromises()
  }

  it('点击「记一笔」主体直接打开「记一笔 · 支出」弹窗，仅渲染支出子表单（无类型单选）', async () => {
    const wrapper = await mountView()
    // 初始关闭：无 Modal、无表单
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    expect(wrapper.findComponent(TransactionForm).exists()).toBe(false)
    await openCreateModal(wrapper)
    expect(wrapper.findComponent(NModal).props('show')).toBe(true)
    // 标题标明类型（issue #150）
    expect(wrapper.findComponent(NModal).props('title')).toBe('记一笔 · 支出')
    // 类型由入口单点表达：表单内无类型单选组，按入口 kind 只渲染支出子表单
    const form = wrapper.findComponent(TransactionForm)
    expect(form.exists()).toBe(true)
    expect(form.props('kind')).toBe('expense')
    expect(wrapper.findComponent(NRadioGroup).exists()).toBe(false)
    expect(form.findComponent(CategoryForm).exists()).toBe(true)
    expect(form.findComponent(TransferForm).exists()).toBe(false)
    expect(form.findComponent(InvestmentForm).exists()).toBe(false)
  })

  it('提交成功后弹窗关闭、回到第 1 页并立即刷新（新记录可见）', async () => {
    const wrapper = await mountView()
    // 先翻到第 2 页再记一笔：成功后应回到第 1 页，确保新记录可见
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    await openCreateModal(wrapper)
    const before = listCalls().length
    // 表单提交成功 → created 事件
    wrapper.findComponent(TransactionForm).vm.$emit('created')
    await flushPromises()
    // 弹窗关闭（naive-ui Modal 关闭后内容保留在 DOM 仅隐藏，与 CategoryEditModal 同模式）
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    // 立即以第 1 页重新查询（筛选条件保留）
    expect(listCalls().length).toBe(before + 1)
    expect(lastListFilter()).toMatchObject({ page: 1 })
  })

  it('真实提交链路：弹窗内填表提交 → create_transaction → 弹窗关闭并刷新', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    // 主体按钮打开的 expense 弹窗 → CategoryForm；弹窗表单在 TransactionForm 子树内定位
    const form = wrapper.findComponent(TransactionForm)
    // 金额（NInputNumber）与账户（CategoryForm 内第 2 个 NSelect，第 1 个是币种）
    form.getComponent(NInputNumber).vm.$emit('update:value', 12.5)
    form.findAllComponents(NSelect)[1].vm.$emit('update:value', 'acc-1')
    await flushPromises()
    mockInvoke.mockImplementationOnce((cmd: string) => {
      if (cmd === 'create_transaction') return Promise.resolve('new-id')
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    // 点击提交按钮「记支出」
    const submitBtn = form
      .findAllComponents(NButton)
      .find((b) => b.text().includes('记支出'))!
    await submitBtn.trigger('click')
    await flushPromises()
    // 后端收到正确账目
    const createCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_transaction')
    expect(createCalls).toHaveLength(1)
    const [, args] = createCalls[0] as [string, { input: Record<string, unknown> }]
    expect(args.input).toMatchObject({
      kind: 'expense',
      amount_cents: 1250,
      currency_code: 'CNY',
      account_id: 'acc-1',
    })
    // 弹窗关闭且列表刷新（回到第 1 页）
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    expect(lastListFilter()).toMatchObject({ page: 1 })
  })

  it('仅关闭弹窗（不提交）不触发列表刷新', async () => {
    const wrapper = await mountView()
    await openCreateModal(wrapper)
    const before = listCalls().length
    // 用户点遮罩/关闭 → update:show=false
    wrapper.findComponent(NModal).vm.$emit('update:show', false)
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    expect(listCalls().length).toBe(before)
  })
})

describe('TransactionsView 记一笔分裂按钮（issue #150）', () => {
  // jsdom 的 document.body 跨测试共享：前序测试遗留的已展开下拉菜单（teleport 到 body、
  // wrapper 未 destroy）会被 querySelector 误命中，先清掉
  beforeEach(() => {
    document.body.innerHTML = ''
  })

  /** 点击下拉箭头展开菜单，返回 document.body 中的菜单项文案列表。 */
  async function openDropdown(wrapper: ReturnType<typeof mount>): Promise<string[]> {
    const arrow = wrapper.find('button[aria-label="更多记账类型"]')
    expect(arrow.exists()).toBe(true)
    await arrow.trigger('click')
    await flushPromises()
    return [...document.body.querySelectorAll('.n-dropdown-option-body__label')].map(
      (el) => el.textContent ?? '',
    )
  }

  /** 点击下拉菜单中指定文案的菜单项（click handler 绑在 option-body 上）。 */
  async function clickDropdownItem(label: string) {
    const item = [...document.body.querySelectorAll('.n-dropdown-option')].find(
      (el) => el.textContent?.trim() === label,
    )
    expect(item, `下拉菜单中应存在「${label}」项`).toBeDefined()
    const body = item!.querySelector('.n-dropdown-option-body') as HTMLElement
    expect(body).toBeDefined()
    body.click()
    await flushPromises()
  }

  it('下拉菜单为 5 项并标注快捷键：支出 a/收入 i/转账 z/买入 b/卖出 s，无退款（issue #150/#153）', async () => {
    const wrapper = await mountView()
    const labels = await openDropdown(wrapper)
    expect(labels).toEqual(['支出 a', '收入 i', '转账 z', '买入 b', '卖出 s'])
    expect(labels).not.toContain('退款')
  })

  it.each([
    ['支出 a', '支出', 'expense'],
    ['收入 i', '收入', 'income'],
    ['转账 z', '转账', 'transfer'],
    ['买入 b', '买入', 'buy'],
    ['卖出 s', '卖出', 'sell'],
  ] as const)('点菜单项「%s」打开对应类型弹窗（无类型单选组）', async (label, kindLabel, kind) => {
    const wrapper = await mountView()
    await openDropdown(wrapper)
    await clickDropdownItem(label)
    expect(wrapper.findComponent(NModal).props('show')).toBe(true)
    expect(wrapper.findComponent(NModal).props('title')).toBe(`记一笔 · ${kindLabel}`)
    const form = wrapper.findComponent(TransactionForm)
    expect(form.props('kind')).toBe(kind)
    expect(wrapper.findComponent(NRadioGroup).exists()).toBe(false)
  })

  it('下拉展开后再点主体，仍直接打开支出弹窗（两击区互不干扰）', async () => {
    const wrapper = await mountView()
    await openDropdown(wrapper)
    const btn = wrapper.findAll('button').find((b) => b.text().includes('记一笔'))!
    await btn.trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('title')).toBe('记一笔 · 支出')
  })
})

