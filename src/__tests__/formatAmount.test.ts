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

  it('CNY 正数（小数尾零裁剪）', () => {
    expect(formatAmount(10000, cny)).toBe('¥100')
  })

  it('CNY 负数（负号与小数规则互不干扰）', () => {
    expect(formatAmount(-5000, cny)).toBe('-¥50')
  })

  it('零值', () => {
    expect(formatAmount(0, cny)).toBe('¥0')
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

  it('大额正确处理（万分位分组）', () => {
    expect(formatAmount(100000000, cny)).toBe('¥100,0000')
  })

  it('负数零位小数', () => {
    expect(formatAmount(-300, jpy)).toBe('-¥300')
  })

  // —— 小数尾零裁剪：98.00 → 98、98.50 → 98.5、0.50 → 0.5 ——

  it('整数金额去尾零并连小数点一起去掉', () => {
    expect(formatAmount(9800, cny)).toBe('¥98')
    expect(formatAmount(500, cny)).toBe('¥5')
  })

  it('非零尾数保留有效小数', () => {
    expect(formatAmount(9850, cny)).toBe('¥98.5')
    expect(formatAmount(50, cny)).toBe('¥0.5')
  })

  it('末位为零的截断', () => {
    expect(formatAmount(9050, cny)).toBe('¥90.5')
    expect(formatAmount(9805, cny)).toBe('¥98.05') // 中间零不受影响
  })

  it('负数金额去尾零', () => {
    expect(formatAmount(-1250, cny)).toBe('-¥12.5')
  })

  it('非 2 位小数币种同样裁剪（KWD 12.340 → 12.34）', () => {
    expect(formatAmount(12340, kwd)).toBe('KD12.34')
    expect(formatAmount(123400, kwd)).toBe('KD123.4')
  })

  // —— 万分位分组矩阵：整数部分从右向左每 4 位一组、半角逗号分隔 ——

  it('≤4 位整数不受影响', () => {
    expect(formatAmount(9999, cny)).toBe('¥99.99') // 99.99 元
    expect(formatAmount(999900, cny)).toBe('¥9999') // 整数=9999，4 位以内不分组
    expect(formatAmount(1000, cny)).toBe('¥10')
    expect(formatAmount(0, jpy)).toBe('¥0')
  })

  it('跨万位边界：9999 → 9999、10000 → 1,0000', () => {
    expect(formatAmount(9999, jpy)).toBe('¥9999')
    expect(formatAmount(10000, jpy)).toBe('¥1,0000')
  })

  it('多位大数正确切组（每 4 位一组）', () => {
    // ¥1234567.89
    expect(formatAmount(123456789, cny)).toBe('¥123,4567.89')
    // ¥1,2345,6789.00 → 尾零裁剪后无小数点
    expect(formatAmount(12345678900, cny)).toBe('¥1,2345,6789')
  })

  it('万分位分组与小数尾零裁剪叠加', () => {
    // ¥12345678.50 → 1234,5678.5
    expect(formatAmount(1234567850, cny)).toBe('¥1234,5678.5')
  })

  it('小数部分不插分隔符', () => {
    expect(formatAmount(1, kwd)).toBe('KD0.001')
    expect(formatAmount(-98765432109, kwd)).toBe('-KD9876,5432.109')
  })

  it('负数分组且负号在最前', () => {
    expect(formatAmount(-123456789, cny)).toBe('-¥123,4567.89')
  })

  it('0 位小数币种先取整再分组', () => {
    expect(formatAmount(1234567, jpy)).toBe('¥123,4567')
  })

  it('2 位小数币种跨万位边界（9999.99 → 10000）', () => {
    expect(formatAmount(999999, cny)).toBe('¥9999.99')
    expect(formatAmount(1000000, cny)).toBe('¥1,0000')
  })
})
