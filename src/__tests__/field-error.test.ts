import { describe, it, expect } from 'vitest'
import { judgeAmountText, judgeRequiredText, fieldErrorKind } from '@/utils/field-error'

/**
 * 字段错误态共享判定单点测试（ADR-0058 / issue #414，规则表接缝）：
 * 穷举金额格式错误闭集（解析失败 / 超出表示精度 / 必填为空）与
 * 错误态装配的时机口径（输入中即红；空值红在失焦或保存尝试后）。
 * 改判定口径只碰 src/utils/field-error.ts 一处，此处同步钉死。
 */

describe('judgeAmountText（金额格式判定闭集）', () => {
  describe('必填为空', () => {
    it('空字符串 → empty', () => {
      expect(judgeAmountText('')).toEqual({ kind: 'empty' })
    })

    it('纯空白（含全角空格外的常见空白）→ empty', () => {
      expect(judgeAmountText('   ')).toEqual({ kind: 'empty' })
      expect(judgeAmountText('\t\n')).toEqual({ kind: 'empty' })
    })
  })

  describe('合法（ok，携带元的数值）', () => {
    it.each([
      ['12', 12],
      ['0', 0],
      ['007', 7],
      ['12.5', 12.5],
      ['12.55', 12.55],
      ['0.05', 0.05],
      ['.5', 0.5],
      ['12.', 12],
      [' 12.5 ', 12.5],
      ['99999999.99', 99999999.99],
    ])('%s → ok %s', (text, yuan) => {
      expect(judgeAmountText(text)).toEqual({ kind: 'ok', yuan })
    })

    it('负数可解析（不属格式错误闭集，业务类校验走提交时既有通道）', () => {
      expect(judgeAmountText('-5')).toEqual({ kind: 'ok', yuan: -5 })
    })
  })

  describe('解析失败（parse-error）', () => {
    it.each([
      '4.30发', // 混排非数字字符（原始诉求）
      'abc',
      '1e3', // 科学计数法（与 yuanToCents 口径一致：拒绝）
      '1,000', // 千分位逗号
      '4.3.0', // 多个小数点
      '１２３', // 全角数字
      '.', // 无数字
      '-', // 只有符号
      '12.5. ',
      '1 2',
    ])('%s → parse-error', (text) => {
      expect(judgeAmountText(text)).toEqual({ kind: 'parse-error' })
    })
  })

  describe('超出表示精度（over-precision，金额以整数分表达、至多两位小数）', () => {
    it.each(['4.305', '0.001', '.505', '12.123', '-.005'])('%s → over-precision', (text) => {
      expect(judgeAmountText(text)).toEqual({ kind: 'over-precision' })
    })
  })
})

describe('judgeRequiredText（必填口径）', () => {
  it('null / undefined / 空串 / 纯空白 → empty', () => {
    expect(judgeRequiredText(null)).toEqual({ kind: 'empty' })
    expect(judgeRequiredText(undefined)).toEqual({ kind: 'empty' })
    expect(judgeRequiredText('')).toEqual({ kind: 'empty' })
    expect(judgeRequiredText('   ')).toEqual({ kind: 'empty' })
  })

  it('非空白文本 → ok', () => {
    expect(judgeRequiredText('现金')).toEqual({ kind: 'ok' })
    expect(judgeRequiredText(' x ')).toEqual({ kind: 'ok' })
  })
})

describe('fieldErrorKind（错误态装配：格式类即时红，空值红在失焦或保存尝试后）', () => {
  const untouched = { touched: false, saveAttempted: false }

  it('合法判定永无错误态（任意时机）', () => {
    expect(fieldErrorKind({ kind: 'ok', yuan: 1 }, untouched)).toBeNull()
    expect(fieldErrorKind({ kind: 'ok', yuan: 1 }, { touched: true, saveAttempted: true })).toBeNull()
  })

  it('解析失败即时红（不待失焦/保存尝试）', () => {
    expect(fieldErrorKind({ kind: 'parse-error' }, untouched)).toBe('parse-error')
  })

  it('超出精度即时红', () => {
    expect(fieldErrorKind({ kind: 'over-precision' }, untouched)).toBe('over-precision')
  })

  it('空值：初始未触碰未尝试 → 不红（不惩罚尚未输入）', () => {
    expect(fieldErrorKind({ kind: 'empty' }, untouched)).toBeNull()
  })

  it('空值：失焦后 → 红', () => {
    expect(fieldErrorKind({ kind: 'empty' }, { touched: true, saveAttempted: false })).toBe('empty')
  })

  it('空值：保存尝试后 → 红（提交意图触发兜底红态）', () => {
    expect(fieldErrorKind({ kind: 'empty' }, { touched: false, saveAttempted: true })).toBe('empty')
  })

  it('空值：失焦 + 保存尝试 → 红', () => {
    expect(fieldErrorKind({ kind: 'empty' }, { touched: true, saveAttempted: true })).toBe('empty')
  })
})
