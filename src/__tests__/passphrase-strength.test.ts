import { describe, it, expect } from 'vitest'
import {
  strengthForScore,
  assessPassphraseStrength,
  type PassphraseStrengthTier,
} from '@/utils/passphrase-strength'

/**
 * 口令强度收口单点测试（词汇表「口令强度」，备份与数据文件域；issue #685）：
 * 钉死 score→档位闭集映射（0–1 弱 / 2 中 / 3 强 / 4 极强）、空输入不评估、
 * 典型样例档位合理（password123 → 弱，长随机串 → 极强）。
 * 改判定口径只碰 src/utils/passphrase-strength.ts 一处，此处同步钉死。
 */

const ALL_TIERS: PassphraseStrengthTier[] = ['weak', 'medium', 'strong', 'very-strong']

describe('strengthForScore（score→档位闭集映射）', () => {
  it('score 0–1 → weak（闭集两端同档）', () => {
    expect(strengthForScore(0).tier).toBe('weak')
    expect(strengthForScore(1).tier).toBe('weak')
  })

  it('score 2 → medium / 3 → strong / 4 → very-strong', () => {
    expect(strengthForScore(2).tier).toBe('medium')
    expect(strengthForScore(3).tier).toBe('strong')
    expect(strengthForScore(4).tier).toBe('very-strong')
  })

  it('色条填充随 score 单调递增（(score+1)/20%），同档内保留刻度差', () => {
    const percents = ([0, 1, 2, 3, 4] as const).map((s) => strengthForScore(s).percent)
    expect(percents).toEqual([20, 40, 60, 80, 100])
  })

  it('档位闭集四值，评估结果不落域外', () => {
    for (const score of [0, 1, 2, 3, 4] as const) {
      expect(ALL_TIERS).toContain(strengthForScore(score).tier)
      expect(strengthForScore(score).score).toBe(score)
    }
  })
})

describe('assessPassphraseStrength（zxcvbn 评估入口）', () => {
  it('空输入 → null（初始为空不显示，沿字段错误态「不惩罚尚未输入」精神）', async () => {
    expect(await assessPassphraseStrength('')).toBeNull()
  })

  it('password123 → weak（典型弱口令样例）', async () => {
    const result = await assessPassphraseStrength('password123')
    expect(result?.tier).toBe('weak')
  })

  it('长随机串 → very-strong（典型强口令样例）', async () => {
    const result = await assessPassphraseStrength('qW7#mKx2$vLp9&zR4')
    expect(result?.tier).toBe('very-strong')
  })

  it('结果始终落在四档闭集内（中间强度样例不落域外）', async () => {
    for (const sample of ['Tr0ub4dour&3', 'correct-horse', '主口令至少八个字']) {
      const result = await assessPassphraseStrength(sample)
      expect(result, `样例 ${sample} 应有评估结果`).not.toBeNull()
      expect(ALL_TIERS).toContain(result!.tier)
    }
  })
})
