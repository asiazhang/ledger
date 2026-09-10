import { describe, it, expect, afterEach } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { NDataTable, NModal, NSelect, NPagination } from 'naive-ui'
// 路由替身经 common.ts 的 vi.mock 注册，必须先于任何直连组件导入（导入顺序即 mock 生效面）
import {
  mountView, routeMock, makeTxn, setTxnDb,
  lastListFilter, rowMenu, rowMenuKeys, selectRowMenu,
} from './common'
import { setFakeMedia } from '../helpers/media-mock'
import { clickDialogButton } from '../helpers/dom'
import { fireProp } from '../helpers/component-vm'
import { amountPrivacyEnabled, formatAmount } from '@/utils/money'
import { kindSemanticColor } from '@/theme/semantic-colors'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { refCurrencies } from '../helpers/reference-stubs'
import AccountLink from '@/components/AccountLink.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import TransactionCardList from '@/components/TransactionCardList.vue'

/**
 * 交易页移动档（issue #846 / ADR-0088 决策 9 断点双渲染）：组件测试主接缝。
 * 换档一律经媒体查询测试接缝（helpers/media-mock）：默认桌面指针态为桌面档基线；
 * `width: 839` 换移动档（输入轴不变——桌面缩窗是移动档的附带能力），
 * `width: 839 + hover: none + pointer: coarse` 为触屏手机形态。断言「看到什么、
 * 交互后发生什么」：两档渲染分支、卡片字段与隐私掩码、卡片「⋯」与整卡编辑、
 * FAB → 类型选择 → 表单五类型意图矩阵、分页/筛选/URL 下钻/页码回退两档一致。
 */

const cny = refCurrencies[0]

/** 移动档挂载（宽度轴换档，输入轴保持指针——桌面缩窗基线）。 */
function mountMobile() {
  setFakeMedia({ width: 839 })
  return mountView()
}

/** 触屏手机挂载（移动档 + 触控轴）。 */
function mountPhone() {
  setFakeMedia({ width: 839, hover: 'none', pointer: 'coarse' })
  return mountView()
}

function cards(wrapper: Awaited<ReturnType<typeof mountView>>) {
  return wrapper.findAll('.transaction-card')
}

/** 当前展示中的弹窗（视图有四枚 AppModal，意图非空即显示派生 show）。 */
function shownModal(wrapper: VueWrapper) {
  return wrapper.findAllComponents(NModal).find((m) => m.props('show') === true)
}

/** 关闭展示中弹窗（update:show 装配缝；attrs 同名合并可能为数组，逐个调用）。 */
async function closeShownModal(wrapper: VueWrapper) {
  const modal = shownModal(wrapper)
  expect(modal, '期望有展示中的弹窗').toBeTruthy()
  const handler = modal!.props('onUpdate:show') as unknown as
    | ((v: boolean) => void)
    | Array<(v: boolean) => void>
  for (const fn of Array.isArray(handler) ? handler : [handler]) fn?.(false)
  await flushPromises()
}

/** 语义色内联样式断言：期待值经探针元素走同一 jsdom 归一化，逐字可比。 */
function probeColor(color: string): string {
  const probe = document.createElement('span')
  probe.style.color = color
  return probe.style.color
}

afterEach(() => {
  amountPrivacyEnabled.value = false
})

describe('TransactionsView 断点双渲染（issue #846）', () => {
  it('桌面档：表格在、卡片列表与 FAB 不在（回归红线）', async () => {
    const wrapper = await mountView()
    expect(wrapper.findComponent(NDataTable).exists()).toBe(true)
    expect(wrapper.findComponent(TransactionCardList).exists()).toBe(false)
    expect(wrapper.find('.transaction-card').exists()).toBe(false)
    expect(wrapper.find('.create-fab').exists()).toBe(false)
  })

  it('移动档：卡片列表在、表格与工具栏记一笔按钮不在', async () => {
    const wrapper = await mountMobile()
    expect(wrapper.findComponent(NDataTable).exists()).toBe(false)
    expect(wrapper.findComponent(TransactionCardList).exists()).toBe(true)
    expect(cards(wrapper).length).toBe(20)
    // 记一笔入口分档：工具栏分裂按钮（唯一文本「记一笔」）桌面档在、移动档不在；
    // FAB 只在移动档（触控轴移动档下快捷键不绑，FAB 是唯一记一笔入口）
    expect(wrapper.text()).not.toContain('记一笔')
    expect(wrapper.find('.create-fab').exists()).toBe(true)
  })

  it('跨断点实时换档：839 ↔ 1280 缩放时卡片与表格互斥切换', async () => {
    setFakeMedia({ width: 1280 })
    const wrapper = await mountView()
    expect(wrapper.findComponent(NDataTable).exists()).toBe(true)
    setFakeMedia({ width: 839 })
    await flushPromises()
    expect(wrapper.findComponent(NDataTable).exists()).toBe(false)
    expect(cards(wrapper).length).toBe(20)
    setFakeMedia({ width: 1280 })
    await flushPromises()
    expect(wrapper.findComponent(NDataTable).exists()).toBe(true)
    expect(wrapper.find('.transaction-card').exists()).toBe(false)
  })
})

describe('移动档卡片字段（同一段列表状态）', () => {
  it('卡片呈现日期/类型/分类/商户/账户/金额，金额着收支语义色', async () => {
    setTxnDb([
      makeTxn(1, 'acc-1', {
        kind: 'expense',
        category_id: 'cat-1',
        merchant_id: 'mch-1',
        date: '2026-03-15',
        amount_native_cents: 12345,
      }),
      makeTxn(2, 'acc-2', { kind: 'income', date: '2026-03-16', amount_native_cents: -6789 }),
    ])
    const wrapper = await mountMobile()
    const list = cards(wrapper)
    expect(list.length).toBe(2)
    const first = list[0]
    expect(first.text()).toContain('2026-03-15')
    expect(first.text()).toContain('支出')
    // 分类路径与商户名同表格列口径（reference 单源解析）
    const reference = useReferenceStore()
    expect(first.text()).toContain(reference.categoryPath('cat-1'))
    expect(first.text()).toContain('京东')
    // 账户名可点击（链接语义同表格）
    expect(first.findComponent(AccountLink).exists()).toBe(true)
    expect(first.text()).toContain('现金')
    // 金额：文案走 formatAmount 单源（含隐私掩码位），色走 kindSemanticColor
    const amountEl = first.find('.amount-cell').element as HTMLElement
    expect(amountEl.textContent).toBe(formatAmount(12345, cny))
    expect(amountEl.style.color).toBe(probeColor(kindSemanticColor('expense', useAppStore().theme)))
    // 收入行语义色为收入绿（方向色不因卡片形态丢失）
    const incomeEl = list[1].find('.amount-cell').element as HTMLElement
    expect(incomeEl.style.color).toBe(probeColor(kindSemanticColor('income', useAppStore().theme)))
  })

  it('转账行账户呈现「转出 → 转入」双向链接（与表格同构）', async () => {
    setTxnDb([makeTxn(1, 'acc-1', { kind: 'transfer', to_account_id: 'acc-2' })])
    const wrapper = await mountMobile()
    const first = cards(wrapper)[0]
    const links = first.findAllComponents(AccountLink)
    expect(links.map((l: { text(): string }) => l.text())).toEqual(['现金', '银行'])
    expect(first.text()).toContain('→')
  })

  it('金额隐私模式：隐藏数字不隐藏形状与方向——掩码恒形、语义色保留', async () => {
    setTxnDb([makeTxn(1, 'acc-1', { kind: 'expense', amount_native_cents: 12345 })])
    const wrapper = await mountMobile()
    // 经 store 开关开启（启动水合在 store 首次创建时读取本地存储，与真实使用同路径）
    useAppStore().setAmountPrivacyEnabled(true)
    await flushPromises()
    const amountEl = cards(wrapper)[0].find('.amount-cell').element as HTMLElement
    expect(amountEl.textContent).toBe('••••')
    expect(amountEl.style.color).toBe(probeColor(kindSemanticColor('expense', useAppStore().theme)))
  })

  it('来源行保留链接语义：SourceLink 渲染于卡片', async () => {
    setTxnDb([
      makeTxn(1, 'acc-1', {
        source: { kind: 'subscription', entity_id: 'sub-1', display_name: '视频会员', status: null },
      }),
    ])
    const wrapper = await mountMobile()
    expect(cards(wrapper)[0].text()).toContain('视频会员')
  })

  it('金额触控轴点按查看全文（悬停一击可达在卡片上同规）', async () => {
    setTxnDb([makeTxn(1, 'acc-1', { amount_native_cents: 1234567 })])
    const wrapper = await mountPhone()
    await cards(wrapper)[0].find('.amount-cell').trigger('click')
    await flushPromises()
    const popover = document.body.querySelector('.n-popover')
    expect(popover).not.toBeNull()
    expect(popover!.textContent).toContain(formatAmount(1234567, cny))
  })
})

describe('卡片「⋯」与整卡编辑', () => {
  it('卡片「⋯」点开与桌面右键同一菜单（集合一致）', async () => {
    const wrapper = await mountMobile()
    await cards(wrapper)[0].find('.row-actions-btn').trigger('click')
    await flushPromises()
    expect(rowMenu(wrapper).props('show')).toBe(true)
    // 支出行集合与桌面右键逐项一致（edit/refund/add-item/divider/delete）
    expect(rowMenuKeys(wrapper)).toEqual(['edit', 'refund', 'add-item', 'menu-divider', 'delete'])
    // ⋯ 自身不触发整卡编辑
    expect(shownModal(wrapper)).toBeUndefined()
  })

  it('非支出行「⋯」集合同桌面右键（refund 行仅删除）', async () => {
    setTxnDb([
      makeTxn(1, 'acc-1', { kind: 'transfer', to_account_id: 'acc-2' }),
      makeTxn(2, 'acc-1', { kind: 'refund', refund_of_transaction_id: 'txn-x' }),
    ])
    const wrapper = await mountMobile()
    const list = cards(wrapper)
    await list[0].find('.row-actions-btn').trigger('click')
    await flushPromises()
    expect(rowMenuKeys(wrapper)).toEqual(['edit', 'menu-divider', 'delete'])
    await list[1].find('.row-actions-btn').trigger('click')
    await flushPromises()
    expect(rowMenuKeys(wrapper)).toEqual(['delete'])
  })

  it('整卡点击 = 编辑（refund 行不开放编辑、点击无动作）', async () => {
    setTxnDb([
      makeTxn(1, 'acc-1', { kind: 'expense' }),
      makeTxn(2, 'acc-1', { kind: 'refund', refund_of_transaction_id: 'txn-x' }),
    ])
    const wrapper = await mountMobile()
    const list = cards(wrapper)
    await list[0].trigger('click')
    await flushPromises()
    expect(shownModal(wrapper)?.props('title')).toBe('编辑交易')
    // 关闭后验证 refund 行
    await closeShownModal(wrapper)
    await list[1].trigger('click')
    await flushPromises()
    expect(shownModal(wrapper)).toBeUndefined()
  })

  it('卡内链接点击不触发整卡编辑（账户链接照常下钻）', async () => {
    setTxnDb([makeTxn(1, 'acc-1', { kind: 'expense' })])
    const wrapper = await mountMobile()
    await cards(wrapper)[0].findComponent(AccountLink).find('button').trigger('click')
    await flushPromises()
    expect(shownModal(wrapper)).toBeUndefined()
  })
})

describe('记一笔悬浮按钮（FAB）', () => {
  it('点开五枚大号类型选择（支出/收入/转账/买入/卖出，不含借贷与退款）', async () => {
    const wrapper = await mountPhone()
    await wrapper.find('.create-fab').trigger('click')
    await flushPromises()
    const options = [...document.body.querySelectorAll('.create-fab-option')]
    expect(options.map((o) => o.textContent)).toEqual(['支出', '收入', '转账', '买入', '卖出'])
  })

  it('五类型意图矩阵：类型选择 → 记一笔意图（携带类型）→ 对应表单', async () => {
    const cases = [
      ['支出', '记一笔 · 支出'],
      ['收入', '记一笔 · 收入'],
      ['转账', '记一笔 · 转账'],
      ['买入', '记一笔 · 买入'],
      ['卖出', '记一笔 · 卖出'],
    ] as const
    const wrapper = await mountPhone()
    for (const [optionLabel, expectedTitle] of cases) {
      await wrapper.find('.create-fab').trigger('click')
      await flushPromises()
      const option = [...document.body.querySelectorAll('.create-fab-option')]
        .find((o) => o.textContent === optionLabel)
      expect(option, optionLabel).toBeTruthy()
      option!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await flushPromises()
      const modal = shownModal(wrapper)
      expect(modal, optionLabel).toBeTruthy()
      expect(modal!.props('title'), optionLabel).toBe(expectedTitle)
      // 关窗后再选下一类型（意图替换，序号递增由编排内化）
      await closeShownModal(wrapper)
    }
  })
})

describe('过滤语义两档一致（移动档扩展断言）', () => {
  it('移动档分页：默认 20/页，翻页与页大小切换与桌面同语义', async () => {
    const wrapper = await mountMobile()
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 20 })
    const pagination = wrapper.findComponent(NPagination)
    expect(pagination.exists()).toBe(true)
    fireProp(pagination, 'onUpdate:page', 2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20 })
    expect(cards(wrapper).length).toBe(20)
    // 页大小切换经统一出口：重拉 + 翻回第 1 页
    fireProp(pagination, 'onUpdate:pageSize', 50)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 50 })
    expect(cards(wrapper).length).toBe(45)
  })

  it('移动档筛选：手动筛选立即生效（翻页归零 + 重拉），与桌面同出口', async () => {
    const wrapper = await mountMobile()
    // 账户筛选（PinyinSelect → NSelect 装配缝）→ setFilter 出口：涉及账户语义含转入侧
    const accountSelect = wrapper.findAllComponents(PinyinSelect)[0].findComponent(NSelect)
    fireProp(accountSelect, 'onUpdate:value', 'acc-2')
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 1, involving_account_id: 'acc-2' })
    expect(cards(wrapper).length).toBeGreaterThan(0)
  })

  it('移动档 URL 下钻：?account= 直达过滤（URL 只读入口同一接线）', async () => {
    routeMock.query = { account: 'acc-2' }
    await mountMobile()
    expect(lastListFilter()).toMatchObject({
      page: 1,
      page_size: 20,
      involving_account_id: 'acc-2',
    })
  })

  it('移动档空态：过滤无结果提示 + 清除筛选', async () => {
    setTxnDb([makeTxn(1, 'acc-1', { kind: 'expense' })])
    routeMock.query = { kinds: 'income' }
    const wrapper = await mountMobile()
    expect(cards(wrapper).length).toBe(0)
    expect(wrapper.text()).toContain('没有符合条件的交易')
    await wrapper.findAll('button').find((b) => b.text() === '清除筛选')!.trigger('click')
    await flushPromises()
    expect(cards(wrapper).length).toBe(1)
  })

  it('移动档页码回退：删末页最后一条自动回退上一页（ADR-0045 两档一致）', async () => {
    const wrapper = await mountMobile()
    fireProp(wrapper.findComponent(NPagination), 'onUpdate:page', 3)
    await flushPromises()
    expect(cards(wrapper).length).toBe(5)
    for (let i = 0; i < 5; i++) {
      await cards(wrapper)[0].find('.row-actions-btn').trigger('click')
      await flushPromises()
      await selectRowMenu(wrapper, 'delete')
      await clickDialogButton('删除')
      await flushPromises()
    }
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20 })
    expect(cards(wrapper).length).toBe(20)
  })
})
