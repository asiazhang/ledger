import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount, enableAutoUnmount, flushPromises } from '@vue/test-utils'
import { h, nextTick } from 'vue'
import { NDialogProvider } from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { listen } from '@tauri-apps/api/event'
import { useReferenceStore } from '@/stores/reference'
import { applyLocale } from '@/i18n'
import InvestmentsView from '@/views/InvestmentsView.vue'
import InvestmentForm from '@/components/InvestmentForm.vue'
import { stubReferenceInvoke } from './helpers/reference-stubs'

// 走势图用共享桩组件替代（同 InvestmentsView.test.ts）
vi.mock('vue-chartjs', async () => {
  const { LineChartStub } = await import('./line-chart-stub')
  return { Line: LineChartStub }
})

const mockListen = vi.mocked(listen)

// 英文渲染冒烟（issue #350）：切 en-US 后投资域文案走 en 资源；
// 用例末尾还原 zh-CN，避免污染同进程其他测试（i18n 模块级单例）。
beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  mockListen.mockResolvedValue(() => {})
  stubReferenceInvoke({
    list_accounts: [],
    list_categories: [],
    list_insurers: [],
    list_merchants: [],
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
  })
  localStorage.clear()
  const store = useReferenceStore()
  await store.refresh()
})

enableAutoUnmount(afterEach)
afterEach(async () => {
  document.body.innerHTML = ''
  await applyLocale('zh-CN')
})

function mountView() {
  return mount(NDialogProvider, {
    slots: { default: () => h(InvestmentsView) },
  })
}

async function clickTab(wrapper: ReturnType<typeof mountView>, index: number) {
  await wrapper.findAll('.n-tabs-tab')[index]!.trigger('click')
  await nextTick()
  await nextTick()
}

describe('InvestmentsView 英文渲染（issue #350 / ADR-0049）', () => {
  it('页签渲染英文：P&L / Instruments / Trend', async () => {
    await applyLocale('en-US')
    await nextTick()
    const wrapper = mountView()
    await nextTick()
    const labels = wrapper.findAll('.n-tabs-tab').map((el) => el.text())
    expect(labels).toContain('P&L')
    expect(labels).toContain('Instruments')
    expect(labels).toContain('Trend')
  })

  it('盈亏页持仓概览渲染英文（Current Holdings / 空态）', async () => {
    await applyLocale('en-US')
    const wrapper = mountView()
    await flushPromises()
    expect(wrapper.text()).toContain('Current Holdings')
    expect(wrapper.text()).toContain('Sync Holding Prices')
    // 无持仓数据 → 英文空态
    expect(wrapper.text()).toContain('No holdings')
  })

  it('标的页工具栏与搜索框渲染英文', async () => {
    await applyLocale('en-US')
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, 1)
    expect(wrapper.text()).toContain('Holdings only')
    expect(wrapper.text()).toContain('Add Fund')
    expect(wrapper.text()).toContain('New Instrument')
    expect(wrapper.text()).toContain('Full Sync')
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
    await clickTab(wrapper, 2)
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
