import { describe, it, expect } from 'vitest'
import { installmentSchedule } from '@/utils/installment'

/**
 * 分期每期金额预览纯函数（issue #204）。
 * 口径与后端一致（src-tauri/src/scheduled_transactions/engine.rs expand_occurrences）：
 * floor 均分、尾差进最后一期——不造第二套金额口径。
 */
describe('installmentSchedule 分期每期金额预览', () => {
  it('整除：每期相等，末期无尾差', () => {
    expect(installmentSchedule(1200, 12)).toEqual({ perPeriodCents: 100, lastPeriodCents: 100 })
  })

  it('不整除：floor 均分，尾差进最后一期', () => {
    // 100 分分 3 期 → 33/33/34
    expect(installmentSchedule(100, 3)).toEqual({ perPeriodCents: 33, lastPeriodCents: 34 })
  })

  it('单期：全部金额进最后一期（同时也是第一期）', () => {
    expect(installmentSchedule(999, 1)).toEqual({ perPeriodCents: 999, lastPeriodCents: 999 })
  })

  it('尾差大于单期金额也全部进末期（分 2 期、总额 101）', () => {
    expect(installmentSchedule(101, 2)).toEqual({ perPeriodCents: 50, lastPeriodCents: 51 })
  })

  it('每期之和恒等于总额', () => {
    const { perPeriodCents, lastPeriodCents } = installmentSchedule(1000, 7)
    expect(perPeriodCents * 6 + lastPeriodCents).toBe(1000)
  })

  it('非法输入：总额非正整数抛错', () => {
    expect(() => installmentSchedule(0, 3)).toThrow()
    expect(() => installmentSchedule(-5, 3)).toThrow()
    expect(() => installmentSchedule(10.5, 3)).toThrow()
  })

  it('非法输入：期数小于 1 或非整数抛错', () => {
    expect(() => installmentSchedule(100, 0)).toThrow()
    expect(() => installmentSchedule(100, -1)).toThrow()
    expect(() => installmentSchedule(100, 2.5)).toThrow()
  })
})
