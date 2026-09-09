import { describe, expect, it } from 'vitest'
import { DARK_FLOAT_LAYER, NEUTRAL_TOKENS } from '@/theme/design-tokens'
import { darkOverrides, lightOverrides } from '@/theme/overrides'

/** 中性 Design Tokens 单一来源与派生一致性（issue #887 / ADR-0093）。
 * 行为保持重构：亮暗视觉口径零变化；派生一致性断言防覆盖回退为手工副本。 */

describe('NEUTRAL_TOKENS（中性令牌单一来源）', () => {
  it('暗色定案取值与现状视觉语言一致（近黑底、微分层、细边框、克制圆角）', () => {
    expect(NEUTRAL_TOKENS.dark).toEqual({
      radius: { small: '6px', base: '8px', large: '12px' },
      background: { body: '#0E0E10', card: '#161618', popover: '#1C1C1E', modal: '#1C1C1E' },
      border: { border: 'rgba(255, 255, 255, 0.08)', divider: 'rgba(255, 255, 255, 0.06)' },
      text: { primary: '#ECECEC', secondary: '#A0A0A0', tertiary: '#6E6E6E' },
    })
    expect(DARK_FLOAT_LAYER).toEqual({
      color: '#343438',
      activeOverlay: 'rgba(255, 255, 255, 0.06)',
      activeHoverOverlay: 'rgba(255, 255, 255, 0.08)',
      ringShadow: '0 0 0 1px rgba(255, 255, 255, 0.12), 0 8px 24px rgba(0, 0, 0, 0.5)',
    })
  })

  it('亮/暗两套取值齐全，亮色与暗色取值不同', () => {
    for (const tokens of [NEUTRAL_TOKENS.dark, NEUTRAL_TOKENS.light]) {
      for (const group of Object.values(tokens)) {
        for (const value of Object.values(group)) {
          expect(value).toBeTruthy()
        }
      }
    }
    expect(NEUTRAL_TOKENS.light).not.toEqual(NEUTRAL_TOKENS.dark)
  })
})

describe('darkOverrides 中性常量从 token 派生（防派生断裂为手工副本）', () => {
  const token = NEUTRAL_TOKENS.dark
  const common = darkOverrides.common ?? {}

  it('圆角阶梯与 token 同源', () => {
    expect(common.borderRadius).toBe(token.radius.base)
    expect(common.borderRadiusSmall).toBe(token.radius.small)
    expect(darkOverrides.Card?.borderRadius).toBe(token.radius.large)
    expect(darkOverrides.Input?.borderRadius).toBe(token.radius.base)
    expect(darkOverrides.Menu?.borderRadius).toBe(token.radius.small)
  })

  it('组件级圆角仅保留真实主题变量：Button/Select/DatePicker 无该变量（圆角经 common.borderRadius 传播，无效键已清理）', () => {
    expect(darkOverrides.Button).toBeUndefined()
    expect(darkOverrides.Select).toBeUndefined()
    expect(darkOverrides.DatePicker).toBeUndefined()
  })

  it('背景分层与 token 同源', () => {
    expect(common.bodyColor).toBe(token.background.body)
    expect(common.cardColor).toBe(token.background.card)
    expect(common.popoverColor).toBe(token.background.popover)
    expect(common.modalColor).toBe(token.background.modal)
    expect(darkOverrides.Dropdown?.color).toBe(DARK_FLOAT_LAYER.color)
    expect(darkOverrides.Menu?.itemColorActive).toBe(DARK_FLOAT_LAYER.activeOverlay)
    expect(darkOverrides.Menu?.itemColorActiveHover).toBe(DARK_FLOAT_LAYER.activeHoverOverlay)
  })

  it('边框与浮层 ring 同源', () => {
    expect(common.borderColor).toBe(token.border.border)
    expect(common.dividerColor).toBe(token.border.divider)
    expect(darkOverrides.Dropdown?.peers?.Popover?.boxShadow).toBe(DARK_FLOAT_LAYER.ringShadow)
  })

  it('文字灰阶与 token 同源', () => {
    expect(common.textColor1).toBe(token.text.primary)
    expect(common.textColor2).toBe(token.text.secondary)
    expect(common.textColor3).toBe(token.text.tertiary)
  })

  it('强调色不进 token，仍留在覆盖模块', () => {
    expect(common.primaryColor).toBe('#F59E0B')
  })
})

describe('lightOverrides 亮色保持现状等效（issue #887 边界：不借机补全）', () => {
  it('仅含强调色覆盖，无中性常量', () => {
    expect(Object.keys(lightOverrides).sort()).toEqual(['common'])
    expect(Object.keys(lightOverrides.common ?? {}).sort()).toEqual([
      'primaryColor',
      'primaryColorHover',
      'primaryColorPressed',
      'primaryColorSuppl',
    ])
  })
})
