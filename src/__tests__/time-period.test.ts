import { describe, it, expect, vi } from 'vitest'
import {
  DATED_TIME_PERIOD_PRESETS,
  TIME_PERIOD_PRESETS,
  formatPeriodLabel,
  matchPreset,
  periodRange,
  periodFromTimestamp,
  periodStartTimestamp,
  presetRange,
  rangeToPeriod,
  stepPeriod,
  derivePeriodBoundary,
  deriveAllPeriodBoundaries,
  comparePeriods,
  isPeriodWithinBoundary,
  canStepPeriod,
  clampPeriod,
  type DatedTimePeriodPreset,
  type NaturalPeriod,
  type PeriodBoundary,
} from '@/utils/time-period'

/** 本地日历构造辅助：测试与实现同用本地时区口径，无 UTC 偏移歧义。 */
const d = (y: number, m: number, day: number) => new Date(y, m - 1, day)

describe('time-period 预设闭集', () => {
  it('闭集五项且顺序即芯片渲染顺序：全部 | 当月 | 当季 | 当年 | 去年', () => {
    expect(TIME_PERIOD_PRESETS).toEqual(['all', 'month', 'quarter', 'year', 'lastYear'])
  })

  it('带日期区间的预设子集与全闭集一致（仅缺「全部」）', () => {
    expect(DATED_TIME_PERIOD_PRESETS).toEqual(['month', 'quarter', 'year', 'lastYear'])
  })
})

describe('期间面板时间戳转换', () => {
  it('按本地日历反推月/季/年，并取自然期间起点', () => {
    const timestamp = new Date(2026, 4, 20, 12).getTime()
    expect(periodFromTimestamp('month', timestamp)).toEqual({ unit: 'month', year: 2026, index: 4 })
    expect(periodFromTimestamp('quarter', timestamp)).toEqual({ unit: 'quarter', year: 2026, index: 1 })
    expect(periodFromTimestamp('year', timestamp)).toEqual({ unit: 'year', year: 2026, index: 0 })
    expect(new Date(periodStartTimestamp({ unit: 'quarter', year: 2026, index: 1 }))).toEqual(
      new Date(2026, 3, 1, 12),
    )
  })
})

describe('presetRange：预设 → 含边界日期区间（本地自然周期）', () => {
  it('当月：月中取整月边界', () => {
    expect(presetRange('month', d(2026, 1, 15))).toEqual({ from: '2026-01-01', to: '2026-01-31' })
  })

  it('当月：闰年 2 月含 2 月 29 日', () => {
    expect(presetRange('month', d(2024, 2, 10))).toEqual({ from: '2024-02-01', to: '2024-02-29' })
    expect(presetRange('month', d(2024, 2, 29))).toEqual({ from: '2024-02-01', to: '2024-02-29' })
  })

  it('当月：平年 2 月止于 28 日', () => {
    expect(presetRange('month', d(2023, 2, 28))).toEqual({ from: '2023-02-01', to: '2023-02-28' })
  })

  it('当月：30 天小月与 12 月末', () => {
    expect(presetRange('month', d(2026, 4, 15))).toEqual({ from: '2026-04-01', to: '2026-04-30' })
    expect(presetRange('month', d(2024, 12, 31))).toEqual({ from: '2024-12-01', to: '2024-12-31' })
  })

  it('当季：季初月份 1/4/7/10 分别落到四个自然季度（1–3、4–6、7–9、10–12）', () => {
    expect(presetRange('quarter', d(2026, 1, 1))).toEqual({ from: '2026-01-01', to: '2026-03-31' })
    expect(presetRange('quarter', d(2026, 4, 15))).toEqual({ from: '2026-04-01', to: '2026-06-30' })
    expect(presetRange('quarter', d(2026, 7, 31))).toEqual({ from: '2026-07-01', to: '2026-09-30' })
    expect(presetRange('quarter', d(2026, 10, 1))).toEqual({ from: '2026-10-01', to: '2026-12-31' })
  })

  it('当季：闰年 2 月 29 日仍属一季度，季度边界不受闰月影响', () => {
    expect(presetRange('quarter', d(2024, 2, 29))).toEqual({ from: '2024-01-01', to: '2024-03-31' })
  })

  it('当年：年中、1 月 1 日与 12 月 31 日均取完整自然年', () => {
    expect(presetRange('year', d(2026, 6, 15))).toEqual({ from: '2026-01-01', to: '2026-12-31' })
    expect(presetRange('year', d(2025, 1, 1))).toEqual({ from: '2025-01-01', to: '2025-12-31' })
    expect(presetRange('year', d(2024, 12, 31))).toEqual({ from: '2024-01-01', to: '2024-12-31' })
  })

  it('去年：当前年减一的完整自然年（年初跨年、年末同区间）', () => {
    expect(presetRange('lastYear', d(2026, 1, 1))).toEqual({ from: '2025-01-01', to: '2025-12-31' })
    expect(presetRange('lastYear', d(2026, 12, 31))).toEqual({ from: '2025-01-01', to: '2025-12-31' })
  })

  it('时间戳与 Date 双输入同口径', () => {
    const date = d(2026, 3, 18)
    expect(presetRange('month', date.getTime())).toEqual(presetRange('month', date))
  })
})

describe('matchPreset：当前区间 → 命中预设（高亮派生）', () => {
  const DAILY_PRESETS: readonly DatedTimePeriodPreset[] = DATED_TIME_PERIOD_PRESETS

  it('无日期过滤（双端皆空）= 默认态「全部」点亮', () => {
    expect(matchPreset(null, null, d(2026, 2, 10))).toBe('all')
  })

  it('当前区间恰为某预设定义时命中该预设（字符串区间与换算互逆）', () => {
    const today = d(2026, 2, 10)
    expect(matchPreset('2026-02-01', '2026-02-28', today)).toBe('month')
    expect(matchPreset('2026-01-01', '2026-03-31', today)).toBe('quarter')
    expect(matchPreset('2026-01-01', '2026-12-31', today)).toBe('year')
    expect(matchPreset('2025-01-01', '2025-12-31', today)).toBe('lastYear')
    // 与 presetRange 换算互逆：四个日期预设各自喂回均命中自身
    for (const p of DAILY_PRESETS) {
      const r = presetRange(p, today)
      expect(matchPreset(r.from, r.to, today)).toBe(p)
    }
  })

  it('单端过滤（只起或只止）不命中任何预设', () => {
    expect(matchPreset('2026-02-01', null, d(2026, 2, 10))).toBeNull()
    expect(matchPreset(null, '2026-02-28', d(2026, 2, 10))).toBeNull()
  })

  it('非自然周期边界的任意区间不命中（搜索页任意区间退路的区分度）', () => {
    expect(matchPreset('2026-01-05', '2026-01-20', d(2026, 2, 10))).toBeNull()
  })

  it('跨月/季/年后旧区间不再命中当期预设（高亮熄灭，列表快照不漂移）', () => {
    // 1 月的「当月」区间，进入 2 月后不再点亮
    expect(matchPreset('2026-01-01', '2026-01-31', d(2026, 2, 1))).toBeNull()
    // 历史季度/年份同理
    expect(matchPreset('2025-10-01', '2025-12-31', d(2026, 2, 1))).toBeNull()
    expect(matchPreset('2025-01-01', '2025-12-31', d(2027, 3, 15))).toBeNull()
  })

  it('五个预设的区间两两互异（单选语义：任一区间至多点亮一枚芯片）', () => {
    const today = d(2026, 2, 10)
    const ranges = DAILY_PRESETS.map((p) => presetRange(p, today))
    const keys = new Set(ranges.map((r) => `${r.from}..${r.to}`))
    expect(keys.size).toBe(DAILY_PRESETS.length)
  })
})

describe('rangeToPeriod ⇄ periodRange：区间 ⇄（单位，期间）双向换算（issue #383）', () => {
  it('各期间换算出区间后反推回同一期间（月/季/年全枚举 round-trip）', () => {
    const periods: NaturalPeriod[] = []
    for (let y = 2023; y <= 2027; y++) {
      for (let m = 0; m < 12; m++) periods.push({ unit: 'month', year: y, index: m })
      for (let q = 0; q < 4; q++) periods.push({ unit: 'quarter', year: y, index: q })
      periods.push({ unit: 'year', year: y, index: 0 })
    }
    for (const p of periods) {
      const r = periodRange(p)
      expect(rangeToPeriod(r.from, r.to), JSON.stringify(p)).toEqual(p)
    }
  })

  it('反推唯一性：月/季/年区间两两互异（同一区间不命中两种单位）', () => {
    const keys = new Set<string>()
    for (let y = 2023; y <= 2027; y++) {
      for (let m = 0; m < 12; m++) {
        const r = periodRange({ unit: 'month', year: y, index: m })
        keys.add(`${r.from}..${r.to}`)
      }
      for (let q = 0; q < 4; q++) {
        const r = periodRange({ unit: 'quarter', year: y, index: q })
        keys.add(`${r.from}..${r.to}`)
      }
      const y1 = periodRange({ unit: 'year', year: y, index: 0 })
      keys.add(`${y1.from}..${y1.to}`)
    }
    expect(keys.size).toBe(5 * (12 + 4 + 1))
  })

  it('具体区间反推：月/季/年与跨月历史周期各命中正确单位', () => {
    expect(rangeToPeriod('2026-02-01', '2026-02-28')).toEqual({ unit: 'month', year: 2026, index: 1 })
    expect(rangeToPeriod('2024-02-01', '2024-02-29')).toEqual({ unit: 'month', year: 2024, index: 1 })
    expect(rangeToPeriod('2026-01-01', '2026-03-31')).toEqual({ unit: 'quarter', year: 2026, index: 0 })
    expect(rangeToPeriod('2025-10-01', '2025-12-31')).toEqual({ unit: 'quarter', year: 2025, index: 3 })
    expect(rangeToPeriod('2026-01-01', '2026-12-31')).toEqual({ unit: 'year', year: 2026, index: 0 })
    expect(rangeToPeriod('2025-01-01', '2025-12-31')).toEqual({ unit: 'year', year: 2025, index: 0 })
  })

  it('不可反推区间一律 null：「全部」（双端皆空）、单端过滤、任意区间', () => {
    expect(rangeToPeriod(null, null)).toBeNull()
    expect(rangeToPeriod('2026-01-01', null)).toBeNull()
    expect(rangeToPeriod(null, '2026-01-31')).toBeNull()
    expect(rangeToPeriod('2026-01-05', '2026-01-20')).toBeNull()
    expect(rangeToPeriod('2026-01-15', '2026-01-31')).toBeNull()
    expect(rangeToPeriod('2026-02-01', '2026-03-31')).toBeNull()
    expect(rangeToPeriod('2025-11-01', '2026-02-28')).toBeNull()
  })

  it('非法日期不反推（缺位日期与不存在日期防御）', () => {
    expect(rangeToPeriod('20260101', '2026-03-31')).toBeNull()
    expect(rangeToPeriod('2026-02-30', '2026-03-31')).toBeNull()
    expect(rangeToPeriod('2026-13-01', '2026-12-31')).toBeNull()
  })
})

describe('stepPeriod：期间 ±1 步进（issue #383）', () => {
  it('月期间跨年：12 月 → 次年 1 月、1 月 → 上一年 12 月', () => {
    expect(stepPeriod({ unit: 'month', year: 2026, index: 11 }, 1)).toEqual({
      unit: 'month', year: 2027, index: 0,
    })
    expect(stepPeriod({ unit: 'month', year: 2027, index: 0 }, -1)).toEqual({
      unit: 'month', year: 2026, index: 11,
    })
  })

  it('季期间跨年：四季度 → 次年一季度、一季度 → 上一年四季度', () => {
    expect(stepPeriod({ unit: 'quarter', year: 2026, index: 3 }, 1)).toEqual({
      unit: 'quarter', year: 2027, index: 0,
    })
    expect(stepPeriod({ unit: 'quarter', year: 2027, index: 0 }, -1)).toEqual({
      unit: 'quarter', year: 2026, index: 3,
    })
  })

  it('年期间逐年 ±1（不钳制未来）', () => {
    expect(stepPeriod({ unit: 'year', year: 2026, index: 0 }, -1)).toEqual({
      unit: 'year', year: 2025, index: 0,
    })
    expect(stepPeriod({ unit: 'year', year: 2026, index: 0 }, 1)).toEqual({
      unit: 'year', year: 2027, index: 0,
    })
  })

  it('月/季年内回绕与多步、零步：单位不变、只动年或索引', () => {
    expect(stepPeriod({ unit: 'month', year: 2026, index: 0 }, -1)).toEqual({
      unit: 'month', year: 2025, index: 11,
    })
    expect(stepPeriod({ unit: 'quarter', year: 2026, index: 0 }, -1)).toEqual({
      unit: 'quarter', year: 2025, index: 3,
    })
    expect(stepPeriod({ unit: 'month', year: 2026, index: 5 }, 2)).toEqual({
      unit: 'month', year: 2026, index: 7,
    })
    expect(stepPeriod({ unit: 'quarter', year: 2026, index: 1 }, 0)).toEqual({
      unit: 'quarter', year: 2026, index: 1,
    })
  })

  it('步进后换算区间保持自然周期边界（闰年 2 月 ±1 月落点正确）', () => {
    const feb = stepPeriod({ unit: 'month', year: 2024, index: 0 }, 1)
    expect(periodRange(feb)).toEqual({ from: '2024-02-01', to: '2024-02-29' })
    expect(periodRange(stepPeriod(feb, 1))).toEqual({ from: '2024-03-01', to: '2024-03-31' })
    expect(periodRange(stepPeriod(feb, -2))).toEqual({ from: '2023-12-01', to: '2023-12-31' })
  })
})

describe('formatPeriodLabel：期间标签本地化格式化（issue #383）', () => {
  // 测试环境语言恒为 zh-CN（ADR-0049：业务测试不触碰语言状态）
  it('zh-CN：月份用阿拉伯数字、季度用中文数字、年份裸年', () => {
    expect(formatPeriodLabel({ unit: 'month', year: 2026, index: 0 })).toBe('2026年1月')
    expect(formatPeriodLabel({ unit: 'month', year: 2026, index: 11 })).toBe('2026年12月')
    expect(formatPeriodLabel({ unit: 'quarter', year: 2026, index: 0 })).toBe('2026年一季度')
    expect(formatPeriodLabel({ unit: 'quarter', year: 2026, index: 3 })).toBe('2026年四季度')
    expect(formatPeriodLabel({ unit: 'year', year: 2025, index: 0 })).toBe('2025年')
  })

  it('en-US：月份缩写在前、Q+序数季度、裸年（fresh 模块隔离，不污染其他用例）', async () => {
    const originalLang = Object.getOwnPropertyDescriptor(window.navigator, 'language')
    vi.resetModules()
    Object.defineProperty(window.navigator, 'language', { value: 'en-US', configurable: true })
    try {
      const { initAppLocale } = await import('@/i18n')
      await initAppLocale()
      const { formatPeriodLabel: fmt } = await import('@/utils/time-period')
      expect(fmt({ unit: 'month', year: 2026, index: 1 })).toBe('Feb 2026')
      expect(fmt({ unit: 'month', year: 2026, index: 11 })).toBe('Dec 2026')
      expect(fmt({ unit: 'quarter', year: 2026, index: 0 })).toBe('Q1 2026')
      expect(fmt({ unit: 'quarter', year: 2026, index: 3 })).toBe('Q4 2026')
      expect(fmt({ unit: 'year', year: 2025, index: 0 })).toBe('2025')
    } finally {
      // 恢复 navigator.language 描述符，避免污染同文件后续用例
      if (originalLang) Object.defineProperty(window.navigator, 'language', originalLang)
    }
  })
})

describe('derivePeriodBoundary & deriveAllPeriodBoundaries：数据期间边界派生单点（issue #390）', () => {
  const today = d(2026, 3, 15) // 2026年3月15日（2026年第 3 月 / 2026年一季度 / 2026年）

  it('空库回退：双端皆 null / undefined / 空对象 / 非法日期一律回退为单「当前期间」', () => {
    const emptyCases = [
      null,
      undefined,
      { min_date: null, max_date: null },
      { min_date: '', max_date: '' },
      { min_date: 'invalid', max_date: 'invalid' },
    ]

    for (const ec of emptyCases) {
      // 月档：[2026-03, 2026-03]
      expect(derivePeriodBoundary('month', ec, today)).toEqual({
        earliest: { unit: 'month', year: 2026, index: 2 },
        latest: { unit: 'month', year: 2026, index: 2 },
      })
      // 季档：[2026 Q1, 2026 Q1]
      expect(derivePeriodBoundary('quarter', ec, today)).toEqual({
        earliest: { unit: 'quarter', year: 2026, index: 0 },
        latest: { unit: 'quarter', year: 2026, index: 0 },
      })
      // 年档：[2026, 2026]
      expect(derivePeriodBoundary('year', ec, today)).toEqual({
        earliest: { unit: 'year', year: 2026, index: 0 },
        latest: { unit: 'year', year: 2026, index: 0 },
      })
    }
  })

  it('今天抬升：最新流水在过去时，最晚期间被「今天」抬升至当期（月/季/年三档）', () => {
    // 历史流水：2023-11-05 至 2025-08-20，今天为 2026-03-15
    const range = { min_date: '2023-11-05', max_date: '2025-08-20' }

    // 月档：最早 2023年11月，最晚 max(2025-08, 2026-03) = 2026年3月
    expect(derivePeriodBoundary('month', range, today)).toEqual({
      earliest: { unit: 'month', year: 2023, index: 10 },
      latest: { unit: 'month', year: 2026, index: 2 },
    })

    // 季档：最早 2023年四季度 (index 3)，最晚 max(2025 Q3, 2026 Q1) = 2026年一季度 (index 0)
    expect(derivePeriodBoundary('quarter', range, today)).toEqual({
      earliest: { unit: 'quarter', year: 2023, index: 3 },
      latest: { unit: 'quarter', year: 2026, index: 0 },
    })

    // 年档：最早 2023年，最晚 max(2025, 2026) = 2026年
    expect(derivePeriodBoundary('year', range, today)).toEqual({
      earliest: { unit: 'year', year: 2023, index: 0 },
      latest: { unit: 'year', year: 2026, index: 0 },
    })
  })

  it('未来流水撑大终点：流水晚于今天时，最晚期间延伸至未来流水所在期间', () => {
    // 流水：2024-02-29（闰年）至 2027-11-10，今天为 2026-03-15
    const range = { min_date: '2024-02-29', max_date: '2027-11-10' }

    // 月档：最早 2024年2月，最晚 2027年11月 (index 10)
    expect(derivePeriodBoundary('month', range, today)).toEqual({
      earliest: { unit: 'month', year: 2024, index: 1 },
      latest: { unit: 'month', year: 2027, index: 10 },
    })

    // 季档：最早 2024年一季度 (index 0)，最晚 2027年四季度 (index 3)
    expect(derivePeriodBoundary('quarter', range, today)).toEqual({
      earliest: { unit: 'quarter', year: 2024, index: 0 },
      latest: { unit: 'quarter', year: 2027, index: 3 },
    })

    // 年档：最早 2024年，最晚 2027年
    expect(derivePeriodBoundary('year', range, today)).toEqual({
      earliest: { unit: 'year', year: 2024, index: 0 },
      latest: { unit: 'year', year: 2027, index: 0 },
    })
  })

  it('单日流水恰为今天：起止均为当前期间', () => {
    const range = { min_date: '2026-03-15', max_date: '2026-03-15' }
    expect(derivePeriodBoundary('month', range, today)).toEqual({
      earliest: { unit: 'month', year: 2026, index: 2 },
      latest: { unit: 'month', year: 2026, index: 2 },
    })
    expect(derivePeriodBoundary('quarter', range, today)).toEqual({
      earliest: { unit: 'quarter', year: 2026, index: 0 },
      latest: { unit: 'quarter', year: 2026, index: 0 },
    })
    expect(derivePeriodBoundary('year', range, today)).toEqual({
      earliest: { unit: 'year', year: 2026, index: 0 },
      latest: { unit: 'year', year: 2026, index: 0 },
    })
  })

  it('时间戳与 Date 双输入口径一致', () => {
    const range = { min_date: '2024-01-01', max_date: '2025-12-31' }
    const fromDate = derivePeriodBoundary('month', range, today)
    const fromTimestamp = derivePeriodBoundary('month', range, today.getTime())
    expect(fromTimestamp).toEqual(fromDate)
  })

  it('deriveAllPeriodBoundaries：一并派生月/季/年三档边界', () => {
    const range = { min_date: '2024-06-01', max_date: '2025-02-15' }
    const all = deriveAllPeriodBoundaries(range, today)
    expect(all.month).toEqual(derivePeriodBoundary('month', range, today))
    expect(all.quarter).toEqual(derivePeriodBoundary('quarter', range, today))
    expect(all.year).toEqual(derivePeriodBoundary('year', range, today))
  })

  it('跨多年全枚举：所有月份与季度单调有序且边界包含正确', () => {
    for (let startY = 2023; startY <= 2026; startY++) {
      for (let endY = startY; endY <= 2027; endY++) {
        const range = { min_date: `${startY}-01-01`, max_date: `${endY}-12-31` }
        const bounds = deriveAllPeriodBoundaries(range, today)
        // 月档
        expect(comparePeriods(bounds.month.earliest, bounds.month.latest)).toBeLessThanOrEqual(0)
        expect(bounds.month.earliest.year).toBe(startY)
        expect(bounds.month.earliest.index).toBe(0)
        expect(bounds.month.latest.year).toBe(Math.max(2026, endY))
        expect(bounds.month.latest.index).toBe(Math.max(2026, endY) === 2026 && endY < 2026 ? 2 : 11)

        // 季档
        expect(comparePeriods(bounds.quarter.earliest, bounds.quarter.latest)).toBeLessThanOrEqual(0)
        expect(bounds.quarter.earliest.year).toBe(startY)
        expect(bounds.quarter.earliest.index).toBe(0)

        // 年档
        expect(comparePeriods(bounds.year.earliest, bounds.year.latest)).toBeLessThanOrEqual(0)
        expect(bounds.year.earliest.year).toBe(startY)
        expect(bounds.year.latest.year).toBe(Math.max(2026, endY))
      }
    }
  })
})

describe('comparePeriods & isPeriodWithinBoundary & clampPeriod：期间比较与边界判定（issue #390）', () => {
  it('comparePeriods：同一单位内跨年、年内索引比较严格一致', () => {
    expect(comparePeriods({ unit: 'month', year: 2026, index: 2 }, { unit: 'month', year: 2026, index: 2 })).toBe(0)
    expect(comparePeriods({ unit: 'month', year: 2025, index: 11 }, { unit: 'month', year: 2026, index: 0 })).toBeLessThan(0)
    expect(comparePeriods({ unit: 'month', year: 2026, index: 5 }, { unit: 'month', year: 2026, index: 3 })).toBeGreaterThan(0)

    expect(comparePeriods({ unit: 'quarter', year: 2025, index: 3 }, { unit: 'quarter', year: 2026, index: 0 })).toBeLessThan(0)
    expect(comparePeriods({ unit: 'quarter', year: 2026, index: 2 }, { unit: 'quarter', year: 2026, index: 2 })).toBe(0)

    expect(comparePeriods({ unit: 'year', year: 2024, index: 0 }, { unit: 'year', year: 2026, index: 0 })).toBeLessThan(0)
    expect(comparePeriods({ unit: 'year', year: 2027, index: 0 }, { unit: 'year', year: 2026, index: 0 })).toBeGreaterThan(0)
  })

  it('isPeriodWithinBoundary：端点包含、超界排除', () => {
    const boundary: PeriodBoundary = {
      earliest: { unit: 'month', year: 2025, index: 5 }, // 2025年6月
      latest: { unit: 'month', year: 2026, index: 2 },   // 2026年3月
    }

    // 边界内（含端点）
    expect(isPeriodWithinBoundary({ unit: 'month', year: 2025, index: 5 }, boundary)).toBe(true)
    expect(isPeriodWithinBoundary({ unit: 'month', year: 2025, index: 11 }, boundary)).toBe(true)
    expect(isPeriodWithinBoundary({ unit: 'month', year: 2026, index: 0 }, boundary)).toBe(true)
    expect(isPeriodWithinBoundary({ unit: 'month', year: 2026, index: 2 }, boundary)).toBe(true)

    // 越界排除
    expect(isPeriodWithinBoundary({ unit: 'month', year: 2025, index: 4 }, boundary)).toBe(false)
    expect(isPeriodWithinBoundary({ unit: 'month', year: 2024, index: 11 }, boundary)).toBe(false)
    expect(isPeriodWithinBoundary({ unit: 'month', year: 2026, index: 3 }, boundary)).toBe(false)
    expect(isPeriodWithinBoundary({ unit: 'month', year: 2027, index: 0 }, boundary)).toBe(false)

    // 异单位防御（不属于同一单位返回 false）
    expect(isPeriodWithinBoundary({ unit: 'quarter', year: 2025, index: 2 }, boundary)).toBe(false)
  })

  it('clampPeriod：低于下界钳制为下界，高于上界钳制为上界，界内原样返回', () => {
    const boundary: PeriodBoundary = {
      earliest: { unit: 'quarter', year: 2024, index: 2 },
      latest: { unit: 'quarter', year: 2026, index: 1 },
    }

    // 界内
    const inRange: NaturalPeriod = { unit: 'quarter', year: 2025, index: 0 }
    expect(clampPeriod(inRange, boundary)).toEqual(inRange)

    // 低于下界
    expect(clampPeriod({ unit: 'quarter', year: 2023, index: 3 }, boundary)).toEqual(boundary.earliest)
    expect(clampPeriod({ unit: 'quarter', year: 2024, index: 1 }, boundary)).toEqual(boundary.earliest)

    // 高于上界
    expect(clampPeriod({ unit: 'quarter', year: 2026, index: 2 }, boundary)).toEqual(boundary.latest)
    expect(clampPeriod({ unit: 'quarter', year: 2027, index: 0 }, boundary)).toEqual(boundary.latest)
  })
})

describe('canStepPeriod：钳制步进辅助与边界可达性判定（issue #390 / #391）', () => {
  const boundary: PeriodBoundary = {
    earliest: { unit: 'month', year: 2025, index: 11 }, // 2025年12月
    latest: { unit: 'month', year: 2026, index: 2 },    // 2026年3月
  }

  it('边界在途/失败时（null / undefined）退化为不钳制，步进恒可达', () => {
    const p: NaturalPeriod = { unit: 'month', year: 2026, index: 2 }
    expect(canStepPeriod(p, -1, null)).toBe(true)
    expect(canStepPeriod(p, 1, null)).toBe(true)
    expect(canStepPeriod(p, -1, undefined)).toBe(true)
    expect(canStepPeriod(p, 1, undefined)).toBe(true)
  })

  it('在最晚边界：可向前步进（-1），不可向后步进（+1）', () => {
    const p: NaturalPeriod = { unit: 'month', year: 2026, index: 2 }
    expect(canStepPeriod(p, -1, boundary)).toBe(true)
    expect(canStepPeriod(p, 1, boundary)).toBe(false)
  })

  it('在最早边界：可向后步进（+1），不可向前步进（-1）', () => {
    const p: NaturalPeriod = { unit: 'month', year: 2025, index: 11 }
    expect(canStepPeriod(p, 1, boundary)).toBe(true)
    expect(canStepPeriod(p, -1, boundary)).toBe(false)
  })

  it('在中间期间：双向均可步进', () => {
    const p: NaturalPeriod = { unit: 'month', year: 2026, index: 0 }
    expect(canStepPeriod(p, -1, boundary)).toBe(true)
    expect(canStepPeriod(p, 1, boundary)).toBe(true)
  })

  it('年界回绕步进的可达性判定（2026年1月向前步进至 2025年12月）', () => {
    const jan2026: NaturalPeriod = { unit: 'month', year: 2026, index: 0 }
    expect(canStepPeriod(jan2026, -1, boundary)).toBe(true) // 2025-12 在边界内

    const tightBoundary: PeriodBoundary = {
      earliest: { unit: 'month', year: 2026, index: 0 },
      latest: { unit: 'month', year: 2026, index: 2 },
    }
    expect(canStepPeriod(jan2026, -1, tightBoundary)).toBe(false) // 2025-12 越界
  })

  it('季与年单位的边界步进判定', () => {
    const qBoundary: PeriodBoundary = {
      earliest: { unit: 'quarter', year: 2025, index: 3 },
      latest: { unit: 'quarter', year: 2026, index: 0 },
    }
    expect(canStepPeriod({ unit: 'quarter', year: 2026, index: 0 }, 1, qBoundary)).toBe(false)
    expect(canStepPeriod({ unit: 'quarter', year: 2026, index: 0 }, -1, qBoundary)).toBe(true)
    expect(canStepPeriod({ unit: 'quarter', year: 2025, index: 3 }, -1, qBoundary)).toBe(false)

    const yBoundary: PeriodBoundary = {
      earliest: { unit: 'year', year: 2024, index: 0 },
      latest: { unit: 'year', year: 2026, index: 0 },
    }
    expect(canStepPeriod({ unit: 'year', year: 2026, index: 0 }, 1, yBoundary)).toBe(false)
    expect(canStepPeriod({ unit: 'year', year: 2026, index: 0 }, -1, yBoundary)).toBe(true)
    expect(canStepPeriod({ unit: 'year', year: 2024, index: 0 }, -1, yBoundary)).toBe(false)
  })
})
