import { describe, it, expect } from 'vitest'
import { formatAmount } from '@/types'
import type { Currency } from '@/types'

const cny: Currency = { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 }
const jpy: Currency = { code: 'JPY', name: '日元', symbol: '¥', decimal_places: 0 }
const usd: Currency = { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 }
const kwd: Currency = { code: 'KWD', name: '科威特第纳尔', symbol: 'KD', decimal_places: 3 }

describe('formatAmount', () => {
  it('默认 2 位小数（无币种时）', () => {
    expect(formatAmount(12345)).toBe('123.45')
  })

  it('CNY 正数', () => {
    expect(formatAmount(10000, cny)).toBe('¥100.00')
  })

  it('CNY 负数', () => {
    expect(formatAmount(-5000, cny)).toBe('-¥50.00')
  })

  it('零值', () => {
    expect(formatAmount(0, cny)).toBe('¥0.00')
  })

  it('JPY 零位小数', () => {
    expect(formatAmount(100, jpy)).toBe('¥100')
  })

  it('USD 两位小数', () => {
    expect(formatAmount(9999, usd)).toBe('$99.99')
  })

  it('KWD 三位小数', () => {
    expect(formatAmount(12345, kwd)).toBe('KD12.345')
  })

  it('大额正确处理', () => {
    expect(formatAmount(100000000, cny)).toBe('¥1000000.00')
  })

  it('负数零位小数', () => {
    expect(formatAmount(-300, jpy)).toBe('-¥300')
  })
})
