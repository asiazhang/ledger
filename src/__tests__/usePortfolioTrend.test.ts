import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import {
  toTrendRange,
  toTrendChartSeries,
  isTrendEmpty,
  hasMarketSource,
  usePortfolioTrend,
} from '@/composables/usePortfolioTrend'
import type { PortfolioValueTrend } from '@/types'
import { makeInstrument } from './factories'

const mockInvoke = vi.mocked(invoke)

const portfolioTrend: PortfolioValueTrend = {
  currency_code: 'CNY',
  points: [
    { date: '2026-06-05', market_value_cents: 100000 },
    { date: '2026-06-12', market_value_cents: 110000 },
    { date: '2026-06-19', market_value_cents: 95000 },
  ],
}

/** 默认 invoke mock：参考数据 + 组合走势 */
function baseInvoke(extra?: Record<string, unknown>) {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies')
      return Promise.resolve([{ code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 }])
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_insurers') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
    if (extra && cmd in extra) {
      const handler = (extra as Record<string, unknown>)[cmd]
      return typeof handler === 'function' ? (handler as () => unknown)() : Promise.resolve(handler)
    }
    if (cmd === 'portfolio_value_trend') return Promise.resolve(portfolioTrend)
    if (cmd === 'list_instruments') return Promise.resolve({ items: [], total: 0 })
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
  const store = useReferenceStore()
  await store.refresh()
})

describe('toTrendRange 预设区间 → 查询区间（纯函数）', () => {
  it('全部 = 不设界（空区间对象）', () => {
    expect(toTrendRange('all', new Date(2026, 7, 27))).toEqual({})
  })

  it('1 月：起点为一个月前同日', () => {
    expect(toTrendRange('1m', new Date(2026, 7, 27))).toEqual({
      start_date: '2026-07-27',
      end_date: null,
    })
  })

  it('3 月：起点为三个月前同日', () => {
    expect(toTrendRange('3m', new Date(2026, 7, 27))).toEqual({
      start_date: '2026-05-27',
      end_date: null,
    })
  })

  it('1 年：起点为一年前同日', () => {
    expect(toTrendRange('1y', new Date(2026, 7, 27))).toEqual({
      start_date: '2025-08-27',
      end_date: null,
    })
  })

  it('月末溢出钳制到目标月最后一日（3-31 减 1 月 → 2-28）', () => {
    expect(toTrendRange('1m', new Date(2026, 2, 31))).toEqual({
      start_date: '2026-02-28',
      end_date: null,
    })
  })

  it('闰日减一年落到平年 2-28（2024-02-29 → 2023-02-28）', () => {
    expect(toTrendRange('1y', new Date(2024, 1, 29))).toEqual({
      start_date: '2023-02-28',
      end_date: null,
    })
  })
})

describe('toTrendChartSeries 点位映射 → 图表数据（纯函数）', () => {
  it('连续周采样映射为 labels + values，label 用真实采样日', () => {
    expect(
      toTrendChartSeries([
        { date: '2026-06-05', value: 100000 },
        { date: '2026-06-12', value: 110000 },
      ]),
    ).toEqual({
      labels: ['2026-06-05', '2026-06-12'],
      values: [100000, 110000],
    })
  })

  it('x 轴按日期连续：缺周生成槽位并填 null（由 spanGaps 连点跨越）', () => {
    expect(
      toTrendChartSeries([
        { date: '2026-06-05', value: 100000 },
        { date: '2026-06-19', value: 95000 },
      ]),
    ).toEqual({
      // 首末点所在周的周一之间逐周生成槽位；中间缺周 label 用周一、值为 null
      labels: ['2026-06-05', '2026-06-08', '2026-06-19'],
      values: [100000, null, 95000],
    })
  })

  it('空点序列 → 空 labels/values', () => {
    expect(toTrendChartSeries([])).toEqual({ labels: [], values: [] })
  })
})

describe('isTrendEmpty 空态判定（纯函数）', () => {
  it('无采样点为空态', () => {
    expect(isTrendEmpty([])).toBe(true)
  })

  it('有采样点非空', () => {
    expect(isTrendEmpty([{ date: '2026-06-05', value: 1 }])).toBe(false)
  })
})

describe('hasMarketSource 行情来源判定（纯函数）', () => {
  it('股票 / ETF（市场已知）有行情来源', () => {
    expect(hasMarketSource({ type: 'stock', market: 'sh' })).toBe(true)
    expect(hasMarketSource({ type: 'etf', market: 'sz' })).toBe(true)
    expect(hasMarketSource({ type: 'stock', market: 'hk' })).toBe(true)
  })

  it('基金 / 债券 / 其他无行情来源', () => {
    expect(hasMarketSource({ type: 'fund', market: 'unknown' })).toBe(false)
    expect(hasMarketSource({ type: 'bond', market: 'unknown' })).toBe(false)
    expect(hasMarketSource({ type: 'other', market: 'unknown' })).toBe(false)
  })

  it('市场未知（无法构造 secid）视为无行情来源', () => {
    expect(hasMarketSource({ type: 'stock', market: 'unknown' })).toBe(false)
  })
})

describe('usePortfolioTrend 走势数据层', () => {
  it('默认组合模式：加载组合市值曲线并映射为图表序列', async () => {
    const { refresh, chartSeries, currencyCode, isEmpty } = usePortfolioTrend()
    await refresh()
    expect(mockInvoke.mock.calls.some(([c]) => c === 'portfolio_value_trend')).toBe(true)
    expect(chartSeries.value).toEqual({
      labels: ['2026-06-05', '2026-06-12', '2026-06-19'],
      values: [100000, 110000, 95000],
    })
    expect(currencyCode.value).toBe('CNY')
    expect(isEmpty.value).toBe(false)
  })

  it('区间切换重新拉取：预设起止日期进入查询参数', async () => {
    const { preset, refresh } = usePortfolioTrend()
    await refresh()
    preset.value = '3m'
    await refresh()
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'portfolio_value_trend')
    // 初始 refresh + preset 变化触发 watch + 显式 refresh
    expect(calls.length).toBeGreaterThanOrEqual(2)
    const last = calls.at(-1)![1] as { filter: { start_date: string; end_date: string | null } }
    expect(last.filter.end_date).toBeNull()
    expect(last.filter.start_date).toMatch(/^\d{4}-\d{2}-\d{2}$/)
  })

  it('单标的模式：以标的 id 调用单标的走势命令，币种取自采样点', async () => {
    baseInvoke({
      instrument_price_trend: {
        instrument_id: 'inst-1',
        points: [
          { date: '2026-06-05', price_cents: 1500, currency_code: 'CNY' },
          { date: '2026-06-12', price_cents: 1600, currency_code: 'CNY' },
        ],
      },
    })
    const { refresh, mode, showInstrument, chartSeries, currencyCode } = usePortfolioTrend()
    await refresh()
    showInstrument(
      makeInstrument({ id: 'inst-1', symbol: '600000', name: '浦发银行', type: 'stock', market: 'sh' }),
    )
    await refresh()
    expect(mode.value).toBe('instrument')
    const call = mockInvoke.mock.calls.filter(([c]) => c === 'instrument_price_trend').at(-1)
    expect(call![1]).toMatchObject({ instrumentId: 'inst-1' })
    expect(chartSeries.value).toEqual({ labels: ['2026-06-05', '2026-06-12'], values: [1500, 1600] })
    expect(currencyCode.value).toBe('CNY')
  })

  it('无历史数据：points 为空 → 空态判定为真', async () => {
    baseInvoke({ portfolio_value_trend: { currency_code: 'CNY', points: [] } })
    const { refresh, isEmpty } = usePortfolioTrend()
    await refresh()
    expect(isEmpty.value).toBe(true)
  })

  it('showPortfolio 切回组合模式', async () => {
    const { mode, showInstrument, showPortfolio } = usePortfolioTrend()
    showInstrument(makeInstrument({ id: 'inst-1', type: 'stock', market: 'sh' }))
    expect(mode.value).toBe('instrument')
    showPortfolio()
    expect(mode.value).toBe('portfolio')
  })

  it('加载完成后 loading 复位；命令异常时 loading 同样复位', async () => {
    baseInvoke({ portfolio_value_trend: () => Promise.reject(new Error('boom')) })
    const { refresh, loading } = usePortfolioTrend()
    await expect(refresh()).rejects.toThrow('boom')
    expect(loading.value).toBe(false)
  })
})
