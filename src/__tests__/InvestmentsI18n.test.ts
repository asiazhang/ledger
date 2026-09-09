import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { wireInvokeSeam } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { nextTick } from 'vue'
import { applyLocale } from '@/i18n'
import { clickTab } from './helpers/dom'
import { mountWithDialog } from './helpers/mount'
import InvestmentsView from '@/views/InvestmentsView.vue'
import InvestmentForm from '@/components/InvestmentForm.vue'

// 走势图用共享桩组件替代（同 InvestmentsView.test.ts）
vi.mock('vue-chartjs', async () => {
  const { LineChartStub } = await import('./line-chart-stub')
  return { Line: LineChartStub }
})

// 视图读 route.query（focus 落点消费，issue #709）：本文件不涉落点，空 query
// 即安全空转（同 InvestmentsView.test.ts 的可控 mockRoute 先例）
vi.mock('vue-router', () => ({
  useRoute: () => ({ query: {} }),
  useRouter: () => ({ push: vi.fn() }),
}))

/** 投资域三命令的空数据契约快照（英文渲染不消费具体数据）。 */
const EMPTY_INVESTMENT_DEFAULTS = {
  list_instruments: { items: [], total: 0 },
  list_holdings: [],
  portfolio_value_trend: { currency_code: 'CNY', points: [] },
  realized_pnl_summary: {
    total_realized_pnl_cents: 0,
    by_year: [],
    by_account: [],
    by_instrument: [],
    details: [],
  },
}

// 英文渲染冒烟（issue #350）：切 en-US 后投资域文案走 en 资源；
// 用例末尾还原 zh-CN，避免污染同进程其他测试（i18n 模块级单例）。
beforeEach(async () => {
  // 参考 store 预载走接缝 opt-in 参数（五个 list 命令由桩层规范夹具兑底）。
  await wireInvokeSeam({ defaults: EMPTY_INVESTMENT_DEFAULTS, refreshReferenceStores: true }).ready
})

afterEach(async () => {
  await applyLocale('zh-CN')
})

/** 视图挂载走共享基座（helpers/mount.ts 单点收口，NDialogProvider 包裹）。 */
const mountView = () => mountWithDialog(InvestmentsView)

describe('InvestmentsView 英文渲染（issue #350 / ADR-0049）', () => {
  it('页签渲染英文：P&L / Holdings / Instruments / Trend', async () => {
    await applyLocale('en-US')
    await nextTick()
    const wrapper = mountView()
    await nextTick()
    const labels = wrapper.findAll('.n-tabs-tab').map((el) => el.text())
    expect(labels).toContain('P&L')
    expect(labels).toContain('Holdings')
    expect(labels).toContain('Instruments')
    expect(labels).toContain('Trend')
  })

  it('持仓页签渲染英文（Current Holdings / 空态，issue #901）', async () => {
    await applyLocale('en-US')
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, 'Holdings')
    expect(wrapper.text()).toContain('Current Holdings')
    expect(wrapper.text()).toContain('Sync Instrument Info')
    // 无持仓数据 → 英文空态
    expect(wrapper.text()).toContain('No holdings')
  })

  it('标的页工具栏与搜索框渲染英文', async () => {
    await applyLocale('en-US')
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, 'Instruments')
    expect(wrapper.text()).toContain('Holdings only')
    expect(wrapper.text()).toContain('Add Instrument')
    // 全量同步入口已退役（issue #698），不再渲染 Full Sync
    expect(wrapper.text()).not.toContain('Full Sync')
    expect(wrapper.find('input[placeholder="Search symbol or name..."]').exists()).toBe(true)
    // 空表列头英文
    const headers = wrapper.findAll('th').map((th) => th.text())
    expect(headers).toContain('Symbol')
    expect(headers).toContain('Source')
    expect(headers).toContain('Manual Price')
  })

  it('走势页渲染英文（区间预设 1M / All 与空态引导）', async () => {
    await applyLocale('en-US')
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, 'Trend')
    const text = wrapper.text()
    expect(text).toContain('Portfolio Value')
    expect(text).toContain('Single Instrument')
    expect(text).toContain('1M')
    expect(text).toContain('All')
    // 组合走势空数据 → 英文引导文案
    expect(text).toContain('No historical price data')
  })

  it('投资表单渲染英文 label 与占位', async () => {
    await applyLocale('en-US')
    const wrapper = mount(InvestmentForm, {
      props: { kind: 'buy', submitLabel: '记买入' },
    })
    const labels = wrapper.findAll('.n-form-item-label').map((el) => el.text())
    expect(labels).toContain('Amount')
    expect(labels).toContain('Investment Account')
    expect(labels).toContain('Instrument')
    // NSelect 未展开时 placeholder 呈现在 selection 占位元素而非 input 属性
    expect(wrapper.text()).toContain('Select investment account')
  })
})
