import { describe, expect, it } from 'vitest'
import {
  KIND_SEMANTIC_COLORS,
  kindSemanticColor,
  SEMANTIC_COLOR_KINDS,
} from '@/theme/semantic-colors'
import { TRANSACTION_KINDS } from '@/types'

/** 语义色单一来源（issue #435）：六种交易类型 × 亮/暗两套色值。
 * 只测外部行为：类型穷尽、色值齐全、六色两两不同（防撞色回归）。 */

describe('KIND_SEMANTIC_COLORS（交易类型语义色表）', () => {
  it('类型穷尽：与交易类型闭集一一对应，不多不少', () => {
    expect([...SEMANTIC_COLOR_KINDS].sort()).toEqual([...TRANSACTION_KINDS].sort())
  })

  it('色值齐全：每类型亮/暗两套均为非空色值', () => {
    for (const kind of TRANSACTION_KINDS) {
      const color = KIND_SEMANTIC_COLORS[kind]
      expect(color.light, `${kind}.light`).toMatch(/^#[0-9a-f]{6}$/)
      expect(color.dark, `${kind}.dark`).toMatch(/^#[0-9a-f]{6}$/)
    }
  })

  it('亮色六色两两不同', () => {
    const lights = TRANSACTION_KINDS.map((k) => KIND_SEMANTIC_COLORS[k].light)
    expect(new Set(lights).size).toBe(6)
  })

  it('暗色六色两两不同', () => {
    const darks = TRANSACTION_KINDS.map((k) => KIND_SEMANTIC_COLORS[k].dark)
    expect(new Set(darks).size).toBe(6)
  })

  it('亮暗两套彼此不同（同类型暗色变体确实是变体）', () => {
    for (const kind of TRANSACTION_KINDS) {
      expect(KIND_SEMANTIC_COLORS[kind].light, kind).not.toBe(KIND_SEMANTIC_COLORS[kind].dark)
    }
  })

  it('定案色值（亮色 / 暗色）与 issue #435 决策一致', () => {
    expect(KIND_SEMANTIC_COLORS.expense).toEqual({ light: '#d03050', dark: '#e88080' })
    expect(KIND_SEMANTIC_COLORS.income).toEqual({ light: '#18a058', dark: '#63e2b7' })
    expect(KIND_SEMANTIC_COLORS.refund).toEqual({ light: '#2080f0', dark: '#63a8f2' })
    expect(KIND_SEMANTIC_COLORS.transfer).toEqual({ light: '#722ed1', dark: '#b37feb' })
    expect(KIND_SEMANTIC_COLORS.buy).toEqual({ light: '#eb2f96', dark: '#ff85c0' })
    expect(KIND_SEMANTIC_COLORS.sell).toEqual({ light: '#13c2c2', dark: '#5cdbd3' })
  })
})

describe('kindSemanticColor（按类型与主题取语义色）', () => {
  it.each(TRANSACTION_KINDS)('%s：暗色取 dark、亮色取 light', (kind) => {
    expect(kindSemanticColor(kind, 'dark')).toBe(KIND_SEMANTIC_COLORS[kind].dark)
    expect(kindSemanticColor(kind, 'light')).toBe(KIND_SEMANTIC_COLORS[kind].light)
  })
})
