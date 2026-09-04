import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import {
  formatAmount,
  formatPrice,
  formatQuantity,
  centsToYuan,
  priceToYuan,
  yuanToCents,
  yuanToPrice,
  amountPrivacyEnabled,
} from '@/utils/money'
import type { Currency } from '@/types'

const cny: Currency = { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 }
const jpy: Currency = { code: 'JPY', name: '日元', symbol: '¥', decimal_places: 0 }
const kwd: Currency = { code: 'KWD', name: '科威特第纳尔', symbol: 'KD', decimal_places: 3 }

const MASK = '••••'

describe('金额隐私模式：展示格式化层收口（issue #566）', () => {
  beforeEach(() => {
    amountPrivacyEnabled.value = false
  })
  afterEach(() => {
    amountPrivacyEnabled.value = false
  })

  it('默认关闭', () => {
    expect(amountPrivacyEnabled.value).toBe(false)
  })

  describe('开启：掩码恒等性（任意输入同形输出）', () => {
    beforeEach(() => {
      amountPrivacyEnabled.value = true
    })

    it('formatAmount 对正/负/零、各币种小数位、双语、各数量级恒返回掩码', () => {
      const cases: Array<[number, Currency | undefined, 'zh-CN' | 'en-US']> = [
        [0, undefined, 'zh-CN'],
        [1, undefined, 'en-US'],
        [12345, cny, 'zh-CN'],
        [-12345, cny, 'zh-CN'],
        [100, jpy, 'en-US'],
        [-100, jpy, 'en-US'],
        [12345, kwd, 'en-US'],
        [-98765432109, kwd, 'zh-CN'],
        [12345678900, cny, 'en-US'],
        [-123456789, cny, 'en-US'],
      ]
      for (const [cents, currency, locale] of cases) {
        expect(formatAmount(cents, currency, locale)).toBe(MASK)
      }
    })

    it('formatPrice（投资单价）恒返回掩码', () => {
      for (const price of [0, 12345, -12345, 1234567890, -1500000]) {
        for (const locale of ['zh-CN', 'en-US'] as const) {
          expect(formatPrice(price, cny, locale)).toBe(MASK)
          expect(formatPrice(price, undefined, locale)).toBe(MASK)
        }
      }
    })

    it('formatQuantity（份额数量）恒返回掩码', () => {
      for (const qty of [0, 100, 12345.67, 1234567, -1234.5678]) {
        for (const locale of ['zh-CN', 'en-US'] as const) {
          expect(formatQuantity(qty, locale)).toBe(MASK)
        }
      }
    })

    it('掩码定长 4、无负号、无币种符号（负值与正值同形）', () => {
      expect(MASK).toHaveLength(4)
      expect(formatAmount(-123456789, cny)).toBe(formatAmount(123456789, cny))
      expect(formatPrice(-1234567890, kwd)).toBe(formatPrice(1234567890, kwd))
    })

    it('数值换算出口不受掩码影响（表单正在输入/回填的金额天然例外）', () => {
      expect(centsToYuan(12345, cny)).toBe(123.45)
      expect(yuanToCents('15.5')).toBe(1550)
      expect(priceToYuan(12345)).toBe(1.2345)
      expect(yuanToPrice('1.2345')).toBe(12345)
    })
  })

  describe('关闭：输出与现状逐字符一致（回归保障）', () => {
    it('开启再关闭后，输出恢复与掩码开启前逐字符一致', () => {
      const before = [
        formatAmount(12345, cny),
        formatAmount(-123456789, cny, 'en-US'),
        formatPrice(12345, cny),
        formatPrice(1234567890, cny, 'en-US'),
        formatQuantity(12345.67),
        formatQuantity(-1234567, 'en-US'),
      ]
      amountPrivacyEnabled.value = true
      expect(formatAmount(12345, cny)).toBe(MASK)
      amountPrivacyEnabled.value = false
      expect(formatAmount(12345, cny)).toBe('¥123.45')
      expect(formatAmount(-123456789, cny, 'en-US')).toBe('-¥1,234,567.89')
      expect(formatPrice(12345, cny)).toBe('¥1.2345')
      expect(formatPrice(1234567890, cny, 'en-US')).toBe('¥123,456.789')
      expect(formatQuantity(12345.67)).toBe('1,2345.67')
      expect(formatQuantity(-1234567, 'en-US')).toBe('-1,234,567')
      expect(formatAmount(12345, cny)).toBe(before[0])
      expect(formatQuantity(12345.67)).toBe(before[4])
    })
  })
})
