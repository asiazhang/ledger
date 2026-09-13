import { describe, it, expect, vi, afterEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import {
  HOLDINGS_SEARCH_DEBOUNCE_MS,
  TREND_MODE_DEFAULT,
  TREND_PRESET_DEFAULT,
  useInvestmentsSessionStore,
} from '@/stores/investments-session'
import { makeInstrument } from './factories'

afterEach(() => {
  vi.useRealTimers()
})

describe('useInvestmentsSessionStore（issue #1192 投资页会话状态）', () => {
  it('冷启动默认：默认页签、无持仓筛选/排序、第 1 页、组合走势默认区间', () => {
    const store = useInvestmentsSessionStore()
    expect(store.activeTab).toBe('pnl')
    expect(store.holdingsSearchInput).toBe('')
    expect(store.holdingsSearch).toBe('')
    expect(store.holdingsAccountId).toBeNull()
    expect(store.holdingsSorter).toBeNull()
    expect(store.holdingsPage).toBe(1)
    expect(store.trendMode).toBe(TREND_MODE_DEFAULT)
    expect(store.trendPreset).toBe(TREND_PRESET_DEFAULT)
    expect(store.trendInstrumentId).toBeNull()
    expect(store.trendInstrument).toBeNull()
  })

  it('会话内保留、冷启动回默认（新 pinia 回默认，同 pinia 保留）', () => {
    const store = useInvestmentsSessionStore()
    store.setActiveTab('holdings')
    store.setAccount('acc-1')
    store.setSorter({ columnKey: 'market_value', order: 'descend' })
    store.setPage(3)
    expect(useInvestmentsSessionStore().activeTab).toBe('holdings')
    expect(useInvestmentsSessionStore().holdingsAccountId).toBe('acc-1')
    expect(useInvestmentsSessionStore().holdingsSorter).toEqual({
      columnKey: 'market_value',
      order: 'descend',
    })
    expect(useInvestmentsSessionStore().holdingsPage).toBe(3)

    // 新 pinia = 冷启动：全部回默认
    setActivePinia(createPinia())
    const cold = useInvestmentsSessionStore()
    expect(cold.activeTab).toBe('pnl')
    expect(cold.holdingsAccountId).toBeNull()
    expect(cold.holdingsSorter).toBeNull()
    expect(cold.holdingsPage).toBe(1)
  })

  it('搜索防抖：输入回显即时、应用值 300ms 后生效；翻页归零随应用时点', () => {
    vi.useFakeTimers()
    const store = useInvestmentsSessionStore()
    store.setPage(2)
    store.setSearch('600')
    expect(store.holdingsSearchInput).toBe('600')
    // 防抖窗口内应用值未变、页码不归零
    expect(store.holdingsSearch).toBe('')
    expect(store.holdingsPage).toBe(2)
    vi.advanceTimersByTime(HOLDINGS_SEARCH_DEBOUNCE_MS)
    expect(store.holdingsSearch).toBe('600')
    expect(store.holdingsPage).toBe(1)
  })

  it('排序同值重设不归零；实际变化（含第三态清除）归零第一页', () => {
    const store = useInvestmentsSessionStore()
    store.setSorter({ columnKey: 'unrealized_pnl', order: 'ascend' })
    store.setPage(2)
    store.setSorter({ columnKey: 'unrealized_pnl', order: 'ascend' })
    expect(store.holdingsPage).toBe(2)
    // order=false（naive-ui 第三态）清除排序：属实际变化
    store.setSorter({ columnKey: 'unrealized_pnl', order: false })
    expect(store.holdingsSorter).toBeNull()
    expect(store.holdingsPage).toBe(1)
  })

  it('走势入口：带入标的即选中并切单标的模式；投影可读、面板下拉切换同理', () => {
    const store = useInvestmentsSessionStore()
    const inst = makeInstrument({ id: 'inst-1', symbol: '600000', name: '浦发银行' })
    store.showTrendInstrument(inst)
    expect(store.trendInstrumentId).toBe('inst-1')
    expect(store.trendMode).toBe('instrument')
    expect(store.trendInstrument?.symbol).toBe('600000')
  })

  it('selectTrendInstrument(null) 清除选中并回组合模式（面板清除/切换出口）', () => {
    const store = useInvestmentsSessionStore()
    store.showTrendInstrument(makeInstrument({ id: 'inst-1' }))
    store.selectTrendInstrument(null)
    expect(store.trendInstrumentId).toBeNull()
    expect(store.trendMode).toBe(TREND_MODE_DEFAULT)
    expect(store.trendInstrument).toBeNull()
  })
})

describe('resetToDefault（issue #1192 ESC 复位出口）', () => {
  it('偏离的页签/筛选/排序/页码/走势全部回默认（清除保留态本身）', () => {
    const store = useInvestmentsSessionStore()
    store.setActiveTab('trend')
    store.setSearch('600')
    store.setAccount('acc-1')
    store.setSorter({ columnKey: 'market_value', order: 'descend' })
    store.setPage(3)
    store.showTrendInstrument(makeInstrument({ id: 'inst-1' }))
    store.setTrendPreset('1m')

    store.resetToDefault()
    expect(store.activeTab).toBe('pnl')
    expect(store.holdingsSearchInput).toBe('')
    expect(store.holdingsSearch).toBe('')
    expect(store.holdingsAccountId).toBeNull()
    expect(store.holdingsSorter).toBeNull()
    expect(store.holdingsPage).toBe(1)
    expect(store.trendMode).toBe(TREND_MODE_DEFAULT)
    expect(store.trendPreset).toBe(TREND_PRESET_DEFAULT)
    expect(store.trendInstrumentId).toBeNull()
    expect(store.trendInstrument).toBeNull()
  })

  it('复位撤销在途搜索防抖：复位后旧输入不落地（不留迟到写入）', () => {
    vi.useFakeTimers()
    const store = useInvestmentsSessionStore()
    store.setSearch('600')
    store.resetToDefault()
    // 防抖窗口推进后应用值仍为空：复位已撤销在途定时器
    vi.advanceTimersByTime(HOLDINGS_SEARCH_DEBOUNCE_MS * 2)
    expect(store.holdingsSearch).toBe('')
    expect(store.holdingsSearchInput).toBe('')
  })

  it('默认态复位幂等：全默认时复位无副作用', () => {
    const store = useInvestmentsSessionStore()
    store.resetToDefault()
    expect(store.activeTab).toBe('pnl')
    expect(store.holdingsPage).toBe(1)
    expect(store.trendMode).toBe(TREND_MODE_DEFAULT)
    expect(store.trendInstrument).toBeNull()
  })
})
