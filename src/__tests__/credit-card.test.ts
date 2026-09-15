import { describe, expect, it } from 'vitest'
import {
  CREDIT_UTILIZATION_WARNING_PERCENT,
  availableCreditCents,
  creditProfile,
  creditUtilizationPercent,
  listUtilizationPercent,
  nextOccurrenceOfDay,
  usedCreditCents,
  type CreditAccountFields,
} from '@/accounts/credit-card'

/** 本地日历日构造（与 `date.test.ts` 同纪律：本地构造，跨时区稳定）。 */
const d = (year: number, month1: number, day: number) => new Date(year, month1 - 1, day)

/** 信用卡账户窄口（默认招行卡：额度 50000 元、账单日 5、还款日 25）。 */
function card(patch: Partial<CreditAccountFields> = {}): CreditAccountFields {
  return {
    type: 'credit',
    credit_limit_cents: 5_000_000,
    statement_day: 5,
    due_day: 25,
    ...patch,
  }
}

describe('usedCreditCents（已用额度 = max(0, −余额)，spec #1327 / ADR-0119）', () => {
  it.each([
    ['欠款', -320_000, 320_000],
    ['零余额', 0, 0],
    ['溢缴款（余额为正）', 100_00, 0],
  ])('%s：%i 分 → %i 分', (_label, balance, expected) => {
    expect(usedCreditCents(balance)).toBe(expected)
  })
})

describe('availableCreditCents（可用额度 = 额度 − 已用 + 溢缴款 = 额度 + 余额）', () => {
  it.each([
    ['有欠款', -100_00, 5_000_000, 4_990_000],
    ['零余额', 0, 5_000_000, 5_000_000],
    ['溢缴款叠加', 200_00, 5_000_000, 5_020_000],
    ['超额使用（可用为负，如实呈现）', -5_200_000, 5_000_000, -200_000],
  ])('%s：余额 %i 分 + 额度 %i 分 → %i 分', (_label, balance, limit, expected) => {
    expect(availableCreditCents(balance, limit)).toBe(expected)
  })

  it.each([
    ['额度未设置', null],
    ['额度为 0（非法值，视作未设置）', 0],
  ])('%s：返回 null（不猜额度）', (_label, limit) => {
    expect(availableCreditCents(-100_00, limit)).toBeNull()
  })
})

describe('creditUtilizationPercent（已用 / 额度，整数四舍五入）', () => {
  it.each([
    ['整值', -250_00, 100_00, 250],
    ['四舍五入向下', -333_00, 1_000_00, 33],
    ['四舍五入向上', -336_00, 1_000_00, 34],
    ['无欠款', 500_00, 1_000_00, 0],
    ['超额使用（可用为负仍如实报百分比）', -1_200_00, 1_000_00, 120],
  ])('%s：余额 %i 分 / 额度 %i 分 → %i%%', (_label, balance, limit, expected) => {
    expect(creditUtilizationPercent(balance, limit)).toBe(expected)
  })

  it('额度未设置 → null', () => {
    expect(creditUtilizationPercent(-100_00, null)).toBeNull()
  })
})

describe('nextOccurrenceOfDay（下次某日，本地日历日 + 月末钳制）', () => {
  it.each([
    ['今天之前 → 下月同日', 5, d(2026, 9, 14), '2026-10-05'],
    ['今天之后 → 本月同日', 20, d(2026, 9, 14), '2026-09-20'],
    ['今天就是该日 → 取今天', 14, d(2026, 9, 14), '2026-09-14'],
    ['2 月里的 31 日 → 当月最后一天', 31, d(2026, 2, 10), '2026-02-28'],
    ['闰年 2 月里的 31 日 → 29 日', 31, d(2024, 2, 10), '2024-02-29'],
    ['今天已是当月最后一天 → 取今天', 31, d(2026, 2, 28), '2026-02-28'],
    ['跨月不足日 → 下月钳制（1/31 → 2/28）', 30, d(2026, 1, 31), '2026-02-28'],
    ['跨年', 5, d(2026, 12, 20), '2027-01-05'],
    ['11 月底到 12 月（不跨年）', 5, d(2026, 11, 30), '2026-12-05'],
  ])('%s：%i 日 / 今天 %s → %s', (_label, day, today, expected) => {
    expect(nextOccurrenceOfDay(day, today)).toBe(expected)
  })

  it.each([
    ['未设置', null],
    ['0 日越界', 0],
    ['32 日越界', 32],
    ['非整数', 1.5],
  ])('%s → null（不猜、不截断成 1 日）', (_label, day) => {
    expect(nextOccurrenceOfDay(day, d(2026, 9, 14))).toBeNull()
  })
})

describe('creditProfile（信用卡派生视图的唯一入口）', () => {
  it('非信用卡账户返回 null（调用方据此分支，不散落 type 判断）', () => {
    const bank = { ...card(), type: 'bank' } as CreditAccountFields
    expect(creditProfile(bank, -100_00)).toBeNull()
    expect(listUtilizationPercent(bank, -100_00)).toBeNull()
  })

  it('信用卡账户：额度用量与下次账单节点一次算全', () => {
    const profile = creditProfile(card(), -320_000, d(2026, 9, 14))!
    expect(profile.limitCents).toBe(5_000_000)
    expect(profile.usedCents).toBe(320_000)
    expect(profile.availableCents).toBe(4_680_000)
    expect(profile.utilizationPercent).toBe(6)
    expect(profile.nextStatementDate).toBe('2026-10-05')
    expect(profile.nextDueDate).toBe('2026-09-25')
  })

  it('未设额度的信用卡：额度派生为 null，日子照常派生（三个字段彼此独立）', () => {
    const profile = creditProfile(
      card({ credit_limit_cents: null, statement_day: null }),
      -320_000,
      d(2026, 9, 14),
    )!
    expect(profile.limitCents).toBeNull()
    expect(profile.availableCents).toBeNull()
    expect(profile.utilizationPercent).toBeNull()
    expect(profile.usedCents, '已用额度由余额派生，与额度是否设置无关').toBe(320_000)
    expect(profile.nextStatementDate).toBeNull()
    expect(profile.nextDueDate).toBe('2026-09-25')
  })
})

describe('listUtilizationPercent（列表小字判据：已用 > 0 且额度已设置才显示）', () => {
  it.each([
    ['有欠款且设了额度', -320_000, 5_000_000, 6],
    ['已还清（0%）不显示', 0, 5_000_000, null],
    ['溢缴款不显示', 100_00, 5_000_000, null],
    ['未设额度不显示', -320_000, null, null],
  ])('%s', (_label, balance, limit, expected) => {
    expect(listUtilizationPercent(card({ credit_limit_cents: limit }), balance)).toBe(expected)
  })

  it('阈值常量是 90（列表警示色的唯一定义点）', () => {
    expect(CREDIT_UTILIZATION_WARNING_PERCENT).toBe(90)
  })
})
