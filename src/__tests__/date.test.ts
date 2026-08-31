import { describe, it, expect } from 'vitest'
import { todayStr, toLocalDateISO } from '@/utils/date'

describe('toLocalDateISO（本地日历日 → YYYY-MM-DD，issue #214）', () => {
  it('本地构造的日期取本地年月日（Date 输入）', () => {
    // 以本地时区构造 2024-01-15 各时刻，任何时区下本地日历日都是当天
    expect(toLocalDateISO(new Date(2024, 0, 15))).toBe('2024-01-15')
    expect(toLocalDateISO(new Date(2024, 0, 15, 0, 30))).toBe('2024-01-15')
    expect(toLocalDateISO(new Date(2024, 0, 15, 23, 59))).toBe('2024-01-15')
  })

  it('number 时间戳输入与等值 Date 结果一致', () => {
    const d = new Date(2024, 0, 15, 8, 0)
    expect(toLocalDateISO(d.getTime())).toBe(toLocalDateISO(d))
    expect(toLocalDateISO(d.getTime())).toBe('2024-01-15')
  })

  it('本地 0 点附近归属当天（UTC toISOString 切片会漂到前一天/后一天）', () => {
    // 本地 00:30：东八区下 UTC 日期是前一天；本地 20:00：东八区下 UTC 同日，
    // 但西半球时区下 UTC 日期是后一天。本地语义在任意时区都应返回 15 日。
    expect(toLocalDateISO(new Date(2024, 0, 15, 0, 30))).toBe('2024-01-15')
    expect(toLocalDateISO(new Date(2024, 0, 15, 20, 0))).toBe('2024-01-15')
  })

  it('月份与日期补零', () => {
    expect(toLocalDateISO(new Date(2024, 2, 5))).toBe('2024-03-05')
  })
})

describe('todayStr（本地今天）', () => {
  it('与 toLocalDateISO(new Date()) 同口径', () => {
    expect(todayStr()).toBe(toLocalDateISO(new Date()))
    expect(todayStr()).toMatch(/^\d{4}-\d{2}-\d{2}$/)
  })
})
