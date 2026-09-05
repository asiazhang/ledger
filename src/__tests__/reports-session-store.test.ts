import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useReportsSessionStore } from '@/stores/reports-session'

// 固定「今天」= 2026-01-15（本地）：默认「当年」快照派生随之确定
//（ReportsView 测试同款前提），期望年份一律用字面量 2026。
const Y = 2026

beforeEach(() => {
  setActivePinia(createPinia())
  vi.useFakeTimers()
  vi.setSystemTime(new Date(Y, 0, 15, 12, 0, 0))
})

afterEach(() => {
  vi.useRealTimers()
})

describe('useReportsSessionStore（issue #427 报表页会话状态）', () => {
  it('默认期间 = 「当年」自然周期快照（会话首次使用时按当天派生），下钻为基础态', () => {
    const store = useReportsSessionStore()
    expect(store.period).toEqual({ from: `${Y}-01-01`, to: `${Y}-12-31` })
    expect(store.drilledRootId).toBeNull()
  })

  it('设置期间：写入精确快照（步进/面板产出的任意期间同规）', () => {
    const store = useReportsSessionStore()
    store.setPeriod({ from: '2025-12-01', to: '2025-12-31' })
    expect(store.period).toEqual({ from: '2025-12-01', to: '2025-12-31' })
  })

  it('同值守卫：重复设置同段期间不动作（期间不变，已下钻态不被误复位）', () => {
    const store = useReportsSessionStore()
    store.setPeriod({ from: '2025-01-01', to: '2025-12-31' })
    store.setDrilldown('food')
    store.setPeriod({ from: '2025-01-01', to: '2025-12-31' })
    expect(store.period).toEqual({ from: '2025-01-01', to: '2025-12-31' })
    expect(store.drilledRootId).toBe('food')
  })

  it('期间切换复位下钻：设置不同期间时图内下钻回基础态', () => {
    const store = useReportsSessionStore()
    store.setPeriod({ from: '2025-01-01', to: '2025-12-31' })
    store.setDrilldown('food')
    expect(store.drilledRootId).toBe('food')
    store.setPeriod({ from: '2025-02-01', to: '2025-02-28' })
    expect(store.drilledRootId).toBeNull()
    expect(store.period).toEqual({ from: '2025-02-01', to: '2025-02-28' })
  })

  it('下钻读写：setDrilldown 写入一级分类 id，null 回基础态（期间不受牵连）', () => {
    const store = useReportsSessionStore()
    store.setDrilldown('food')
    expect(store.drilledRootId).toBe('food')
    store.setDrilldown(null)
    expect(store.drilledRootId).toBeNull()
    expect(store.period).toEqual({ from: `${Y}-01-01`, to: `${Y}-12-31` })
  })

  it('商户排行 TopN 默认 5（issue #588 闭集二：5/10）', () => {
    const store = useReportsSessionStore()
    expect(store.merchantTopN).toBe(5)
  })

  it('setMerchantTopN 写入档位：期间与下钻不受牵连', () => {
    const store = useReportsSessionStore()
    store.setDrilldown('food')
    store.setMerchantTopN(10)
    expect(store.merchantTopN).toBe(10)
    expect(store.period).toEqual({ from: `${Y}-01-01`, to: `${Y}-12-31` })
    expect(store.drilledRootId).toBe('food')
  })

  it('TopN 会话内保留、冷启动（新 pinia）回默认 5（ADR-0061 同粒度）', () => {
    const store = useReportsSessionStore()
    store.setMerchantTopN(10)
    expect(store.merchantTopN).toBe(10)
    // 同一 pinia（同一会话）内重新取 store：保留选择
    expect(useReportsSessionStore().merchantTopN).toBe(10)
    // 新 pinia = 冷启动：回默认
    setActivePinia(createPinia())
    expect(useReportsSessionStore().merchantTopN).toBe(5)
  })
})
