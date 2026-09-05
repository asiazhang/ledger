import { describe, it, expect } from 'vitest'
import {
  judgeAmountText,
  judgeQuantityText,
  judgePriceText,
  judgeRequiredText,
  judgeMinLengthText,
  fieldErrorKind,
} from '@/utils/field-error'

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

describe('judgeQuantityText（数量格式判定闭集，输入粒度至多四位小数）', () => {
  describe('必填为空', () => {
    it('空字符串 / 纯空白 → empty', () => {
      expect(judgeQuantityText('')).toEqual({ kind: 'empty' })
      expect(judgeQuantityText('  ')).toEqual({ kind: 'empty' })
    })
  })

  describe('合法（ok，携带数值）', () => {
    it.each([
      ['100', 100],
      ['0', 0],
      ['0.5', 0.5],
      ['0.0001', 0.0001],
      ['12.', 12],
      ['.5', 0.5],
      [' 12 ', 12],
    ])('%s → ok %s', (text, value) => {
      expect(judgeQuantityText(text)).toEqual({ kind: 'ok', value })
    })

    it('负数可解析（业务类校验留提交通道）', () => {
      expect(judgeQuantityText('-5')).toEqual({ kind: 'ok', value: -5 })
    })
  })

  describe('解析失败（parse-error）', () => {
    it.each([
      '4.30发',
      'abc',
      '1e3',
      '1,000',
      '4.3.0',
      '.',
      '-',
      '1 2',
    ])('%s → parse-error', (text) => {
      expect(judgeQuantityText(text)).toEqual({ kind: 'parse-error' })
    })
  })

  describe('超出表示精度（over-precision，至多四位小数）', () => {
    it.each(['1.23456', '0.00005', '.12345', '12.12345'])('%s → over-precision', (text) => {
      expect(judgeQuantityText(text)).toEqual({ kind: 'over-precision' })
    })
  })
})

describe('judgePriceText（价格格式判定闭集，至多四位小数）', () => {
  describe('必填为空', () => {
    it('空字符串 / 纯空白 → empty', () => {
      expect(judgePriceText('')).toEqual({ kind: 'empty' })
      expect(judgePriceText('  ')).toEqual({ kind: 'empty' })
    })
  })

  describe('合法（ok，携带元的数值）', () => {
    it.each([
      ['12', 12],
      ['1.5', 1.5],
      ['0.0001', 0.0001],
      ['12.', 12],
      ['.5', 0.5],
      [' 1.5 ', 1.5],
    ])('%s → ok %s', (text, yuan) => {
      expect(judgePriceText(text)).toEqual({ kind: 'ok', yuan })
    })

    it('负数可解析（业务类校验留提交通道）', () => {
      expect(judgePriceText('-1')).toEqual({ kind: 'ok', yuan: -1 })
    })
  })

  describe('解析失败（parse-error）', () => {
    it.each(['1.23元', 'abc', '1e3', '1,000', '1.2.3', '.', '-'])('%s → parse-error', (text) => {
      expect(judgePriceText(text)).toEqual({ kind: 'parse-error' })
    })
  })

  describe('超出表示精度（over-precision，至多四位小数）', () => {
    it.each(['1.23456', '0.00005', '.10005'])('%s → over-precision', (text) => {
      expect(judgePriceText(text)).toEqual({ kind: 'over-precision' })
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

describe('judgeMinLengthText（最小长度判定，issue #650 主口令 ≥8 首批消费）', () => {
  it('空串 → empty（空值是否红由装配按时机判定，本处不代判）', () => {
    expect(judgeMinLengthText('', 8)).toEqual({ kind: 'empty' })
  })

  it('长度恰为下限 → ok（边界值可提交）', () => {
    expect(judgeMinLengthText('12345678', 8)).toEqual({ kind: 'ok' })
  })

  it('长度超过下限 → ok', () => {
    expect(judgeMinLengthText('123456789', 8)).toEqual({ kind: 'ok' })
  })

  it('短于下限 → too-short（格式类：即时红；不拦截键入由消费方保证）', () => {
    expect(judgeMinLengthText('1234567', 8)).toEqual({ kind: 'too-short' })
  })

  it('空白字符按原样计长（主口令逐字符有效，不做 trim）', () => {
    expect(judgeMinLengthText('  ab    ', 8)).toEqual({ kind: 'ok' })
    expect(judgeMinLengthText(' ab ', 8)).toEqual({ kind: 'too-short' })
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

  it('过短（too-short）即时红（不待失焦/保存尝试，issue #650）', () => {
    expect(fieldErrorKind({ kind: 'too-short' }, untouched)).toBe('too-short')
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
