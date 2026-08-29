import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useReferenceStore } from '@/stores/reference'
import SubscriptionSpendPanel from '@/components/subscriptions/SubscriptionSpendPanel.vue'
import type { SubscriptionSpendOverview, SubscriptionSpendRow } from '@/types'

vi.mock('vue-chartjs', async () => {
  const { BarChartStub } = await import('./line-chart-stub')
  return { Bar: BarChartStub }
})

const mockInvoke = vi.mocked(invoke)
const mockListen = vi.mocked(listen)

enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

const mockCurrencies = [{ code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 }]

/** 生成以 endMonth 结尾的连续 n 个月（YYYY-MM，旧→新） */
function lastMonths(endMonth: string, n: number): string[] {
  const [y, m] = endMonth.split('-').map(Number)
  const months: string[] = []
  for (let i = n - 1; i >= 0; i--) {
    const total = y * 12 + (m - 1) - i
    months.push(`${String(Math.floor(total / 12)).padStart(4, '0')}-${String((total % 12) + 1).padStart(2, '0')}`)
  }
  return months
}

function makeRow(partial: Partial<SubscriptionSpendRow> & { plan_id: string }): SubscriptionSpendRow {
  return {
    note: null,
    merchant_name: null,
    status: 'active',
    amount_cents: 3000,
    currency_code: 'CNY',
    this_month_native_cents: 0,
    this_year_native_cents: 0,
    ...partial,
  }
}

const overview: SubscriptionSpendOverview = {
  native_currency: 'CNY',
  this_month_native_cents: 3000,
  this_year_native_cents: 64800,
  projected_month_native_cents: 4030,
  projected_year_native_cents: 48360,
  months: lastMonths('2026-03', 12).map((month) => ({
    month,
    // 只有 2026-01（年付扣款）与 2026-03（月付）有花费，其余补 0（不摊销口径）
    native_cents: month === '2026-01' ? 34800 : month === '2026-03' ? 3000 : 0,
  })),
  rows: [
    makeRow({
      plan_id: 'sub-1',
      note: '视频会员',
      this_month_native_cents: 3000,
      this_year_native_cents: 3000,
    }),
    makeRow({
      plan_id: 'sub-2',
      note: '已退订服务',
      status: 'cancelled',
      this_month_native_cents: 0,
      this_year_native_cents: 61800,
    }),
  ],
}

function baseInvoke(spend: SubscriptionSpendOverview | Error = overview) {
  mockInvoke.mockImplementation(((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
    if (cmd === 'subscription_spend_overview') {
      return spend instanceof Error ? Promise.reject(spend) : Promise.resolve(spend)
    }
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  }) as typeof invoke)
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  mockListen.mockResolvedValue(() => {})
  baseInvoke()
  const store = useReferenceStore()
  await store.ensureFresh()
})

function chartPayload(wrapper: ReturnType<typeof mount>): {
  labels: string[]
  datasets: { data: number[] }[]
} {
  const el = wrapper.get('[data-testid="bar-chart"]')
  return JSON.parse(el.text())
}

describe('SubscriptionSpendPanel 订阅花费双口径（issue #160/#161）', () => {
  it('渲染本月/本年实际花费汇总（本位币）', async () => {
    const wrapper = mount(SubscriptionSpendPanel)
    await flushPromises()
    expect(wrapper.get('[data-testid="spend-this-month"]').text()).toBe('¥30')
    expect(wrapper.get('[data-testid="spend-this-year"]').text()).toBe('¥648')
    expect(wrapper.text()).toContain('单位：CNY（本位币）· 不摊销')
  })

  it('渲染折算月/年推算成本（只统计进行中订阅，纯展示口径）', async () => {
    const wrapper = mount(SubscriptionSpendPanel)
    await flushPromises()
    expect(wrapper.get('[data-testid="spend-projected-month"]').text()).toBe('¥40.3')
    expect(wrapper.get('[data-testid="spend-projected-year"]').text()).toBe('¥483.6')
    expect(wrapper.text()).toContain('推算：只计进行中的计划')
  })

  it('趋势图渲染过去 12 个月逐月序列（含无扣款月补 0）', async () => {
    const wrapper = mount(SubscriptionSpendPanel)
    await flushPromises()
    const payload = chartPayload(wrapper)
    expect(payload.labels).toEqual(lastMonths('2026-03', 12))
    expect(payload.labels).toHaveLength(12)
    expect(payload.datasets[0].data).toEqual([
      0, 0, 0, 0, 0, 0, 0, 0, 0, 34800, 0, 3000,
    ])
  })

  it('逐订阅行渲染含已取消计划（历史花费如实保留）', async () => {
    const wrapper = mount(SubscriptionSpendPanel)
    await flushPromises()
    const table = wrapper.get('[data-testid="spend-rows"]')
    expect(table.text()).toContain('视频会员')
    expect(table.text()).toContain('已退订服务')
    expect(table.text()).toContain('已取消')
    expect(table.text()).toContain('¥618')
  })

  it('命令失败（如缺汇率中文错误上抛）时显示失败态，不静默混算', async () => {
    baseInvoke(new Error('缺少 USD 兑 CNY 汇率'))
    const wrapper = mount(SubscriptionSpendPanel)
    await flushPromises()
    expect(wrapper.find('[data-testid="spend-failed"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="spend-this-month"]').exists()).toBe(false)
  })
})
