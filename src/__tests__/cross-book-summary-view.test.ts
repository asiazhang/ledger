import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { formatAmount } from '@ledger/money'
import CrossBookSummaryView from '@/views/CrossBookSummaryView.vue'
import type { Currency, CrossBookInvestmentSummary } from '@ledger/types'

// 金额断言委托形态（issue #770）：期待值调同一 formatAmount 实现，带目标币种对象
const CNY: Currency = { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 }

// 跨账本投资汇总页（issue #1196 / ADR-0114）：只读合计视图的组件级行为测试。
// 命令应答走 invoke 测试接缝（ADR-0085）；金额断言委托同一 formatAmount 实现
// （格式规则唯一归属其专测）；折算口径与逐本状态标注是本票的界面义务。

function makeSummary(overrides: Partial<CrossBookInvestmentSummary> = {}): CrossBookInvestmentSummary {
  return {
    target_currency: 'CNY',
    converted: true,
    market_value_cents: 120_000,
    unrealized_pnl_cents: 20_000,
    cumulative_pnl_cents: 45_000,
    investable_assets_cents: 190_000,
    books: [
      { id: 'b1', name: '默认账本', status: 'included' },
      { id: 'b2', name: '美股账本', status: 'included' },
      { id: 'b3', name: '密码本', status: 'locked' },
      { id: 'b4', name: '空账本', status: 'not_initialized' },
    ],
    ...overrides,
  }
}

let payload: CrossBookInvestmentSummary | null = makeSummary()
let fail = false

function wire(): void {
  wireInvokeSeam({
    overrides: {
      cross_book_investment_summary: () => {
        if (fail) throw new Error('未找到 USD -> CNY 的汇率（正反向均无）')
        return payload
      },
    },
  })
}

beforeEach(() => {
  payload = makeSummary()
  fail = false
  wire()
})

afterEach(() => {
  payload = null
})

async function mountView() {
  const wrapper = mount(CrossBookSummaryView)
  await flushPromises()
  return wrapper
}

describe('跨账本投资汇总视图', () => {
  it('渲染四个口径合计（金额经 formatAmount、目标币种符号）', async () => {
    const wrapper = await mountView()
    expect(wrapper.find('[data-testid="summary-marketValue"]').text()).toBe(
      formatAmount(120_000, CNY),
    )
    expect(wrapper.find('[data-testid="summary-unrealizedPnl"]').text()).toBe(
      formatAmount(20_000, CNY),
    )
    expect(wrapper.find('[data-testid="summary-cumulativePnl"]').text()).toBe(
      formatAmount(45_000, CNY),
    )
    expect(wrapper.find('[data-testid="summary-investableAssets"]').text()).toBe(
      formatAmount(190_000, CNY),
    )
  })

  it('发生过折算时标注当期汇率口径（ADR-0114 决策 3 界面义务）', async () => {
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('已按当期汇率折算为 CNY')
    expect(wrapper.text()).not.toContain('直接相加')
  })

  it('位币一致（未折算）时标注直接相加口径', async () => {
    payload = makeSummary({ converted: false })
    wire()
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('直接相加（CNY）')
  })

  it('存在未计入本时警示部分合计，并逐本呈现状态（ADR-0114 决策 5）', async () => {
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('部分账本未计入')
    expect(wrapper.text()).toContain('未解锁、未计入')
    expect(wrapper.text()).toContain('尚未初始化、未计入')
    expect(wrapper.find('[data-testid="summary-book-b3"]').text()).toContain('密码本')
  })

  it('全部计入时不出现部分合计警示', async () => {
    payload = makeSummary({
      books: [
        { id: 'b1', name: '默认账本', status: 'included' },
        { id: 'b2', name: '美股账本', status: 'included' },
      ],
    })
    wire()
    const wrapper = await mountView()
    expect(wrapper.text()).not.toContain('部分账本未计入')
  })

  it('命令报错转 error 兜底显示中文错误，重试可恢复（不空数字、不崩溃）', async () => {
    fail = true
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('汇总读取失败')
    expect(wrapper.text()).toContain('未找到 USD -> CNY 的汇率（正反向均无）')
    expect(wrapper.find('[data-testid="summary-marketValue"]').exists()).toBe(false)

    fail = false
    wire()
    await wrapper.find('button').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="summary-marketValue"]').text()).toBe(
      formatAmount(120_000, CNY),
    )
  })
})

// 接线负向判据（ADR-0087 断言强度）：路由记录是本功能的第二处接线——删除
// routes 中的 cross-book-summary 记录，本断言变红（弹层入口 push 断言覆盖
// 入口侧，本测试覆盖路由侧，两侧删除都有测试承接）。
import { routes } from '@/router/index'

describe('跨账本投资汇总路由接线', () => {
  it('路由表存在 cross-book-summary 记录并指向本视图', () => {
    const route = routes.find((r) => r.name === 'cross-book-summary')
    expect(route).toBeDefined()
    expect(String(route?.path)).toBe('/cross-book-summary')
    expect(String(route?.component)).toContain('CrossBookSummaryView')
  })
})
