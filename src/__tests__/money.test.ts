import { describe, it, expect } from 'vitest'
import { yuanToCents, centsToYuan, formatQuantity, yuanToPrice, priceToYuan } from '@/utils/money'

describe('yuanToCents（元 → 分）', () => {
  it('整数元', () => {
    expect(yuanToCents('15')).toBe(1500)
    expect(yuanToCents('0')).toBe(0)
  })

  it('小数元四舍五入到分：15.5 → 1550', () => {
    expect(yuanToCents('15.5')).toBe(1550)
    expect(yuanToCents('15.50')).toBe(1550)
    expect(yuanToCents('0.01')).toBe(1)
  })

  it('省略整数部分的写法 .5 可用', () => {
    expect(yuanToCents('.5')).toBe(50)
    expect(yuanToCents('-.5')).toBe(-50)
  })

  it('超过两位小数四舍五入', () => {
    expect(yuanToCents('15.505')).toBe(1551)
    expect(yuanToCents('15.504')).toBe(1550)
  })

  it('浮点误差场景：15.505 * 100 在二进制下为 1550.4999…，仍正确舍入', () => {
    expect(yuanToCents('15.505')).toBe(1551)
    expect(yuanToCents('0.1')).toBe(10)
  })

  it('负数支持（筛选金额可为负）', () => {
    expect(yuanToCents('-15.5')).toBe(-1550)
  })

  it('空白与空字符串 → null（不筛选）', () => {
    expect(yuanToCents('')).toBeNull()
    expect(yuanToCents('   ')).toBeNull()
  })

  it('非法输入 → null', () => {
    expect(yuanToCents('abc')).toBeNull()
    expect(yuanToCents('1e3')).toBeNull() // 科学计数法不识别
    expect(yuanToCents('12.34.56')).toBeNull()
    expect(yuanToCents('--5')).toBeNull()
    expect(yuanToCents('15.')).toBeNull() // 小数点后必须有数字
  })

  it('超大数字溢出 → null（不产生 Infinity）', () => {
    expect(yuanToCents('1' + '0'.repeat(308))).toBeNull()
  })
})

describe('yuanToCents（元 → 分）：number 输入分支（issue #214）', () => {
  it('number 与等值 string 结果一致（同一 toFixed(8) 口径，不另写算法）', () => {
    for (const v of [15, 0, 15.5, 0.01, 0.1, -15.5, 12345.67]) {
      expect(yuanToCents(v)).toBe(yuanToCents(String(v)))
    }
  })

  it('15.505 浮点误差边界：15.505 * 100 实为 1550.4999…，仍四舍五入为 1551', () => {
    expect(yuanToCents(15.505)).toBe(1551)
    expect(yuanToCents(15.504)).toBe(1550)
  })

  it('非有限数值 → null', () => {
    expect(yuanToCents(Number.NaN)).toBeNull()
    expect(yuanToCents(Number.POSITIVE_INFINITY)).toBeNull()
    expect(yuanToCents(Number.NEGATIVE_INFINITY)).toBeNull()
  })
})

describe('formatQuantity（数量列万分位分组）', () => {
  it('纯整数从右向左每 4 位一组', () => {
    expect(formatQuantity(12345)).toBe('1,2345')
    expect(formatQuantity(123456789)).toBe('1,2345,6789')
  })

  it('≤4 位整数原样输出', () => {
    expect(formatQuantity(0)).toBe('0')
    expect(formatQuantity(100)).toBe('100')
    expect(formatQuantity(9999)).toBe('9999')
  })

  it('跨万位边界：10000 → 1,0000', () => {
    expect(formatQuantity(10000)).toBe('1,0000')
  })

  it('带小数的份额：整数部分分组、小数部分原样保留', () => {
    expect(formatQuantity(12345.67)).toBe('1,2345.67')
    expect(formatQuantity(123.4507)).toBe('123.4507')
  })
})

describe('centsToYuan（分 → 元，表单初值用数值口径）', () => {
  it('默认 2 位小数币种', () => {
    expect(centsToYuan(3000)).toBe(30)
    expect(centsToYuan(1250)).toBe(12.5)
    expect(centsToYuan(1)).toBe(0.01)
  })

  it('按币种小数位换算', () => {
    expect(centsToYuan(12345, { code: 'JPY', name: '日元', symbol: '¥', decimal_places: 0 })).toBe(
      12345,
    )
    expect(centsToYuan(150, { code: 'KWD', name: '第纳尔', symbol: 'د.ك', decimal_places: 3 })).toBe(
      0.15,
    )
  })

  it('负数与零', () => {
    expect(centsToYuan(0)).toBe(0)
    expect(centsToYuan(-99)).toBe(-0.99)
  })
})

describe('yuanToPrice（元 → 万分之一元，价格列刻度 ADR-0038）', () => {
  it('基金净值 4 位小数无损表示：1.2345 → 12345', () => {
    expect(yuanToPrice('1.2345')).toBe(12345)
    expect(yuanToPrice(1.2345)).toBe(12345)
  })

  it('股票两位价与整数价', () => {
    expect(yuanToPrice('12.34')).toBe(123400)
    expect(yuanToPrice('15')).toBe(150000)
    expect(yuanToPrice(0)).toBe(0)
  })

  it('超过四位小数四舍五入', () => {
    expect(yuanToPrice('1.23456')).toBe(12346)
    expect(yuanToPrice('1.23454')).toBe(12345)
  })

  it('空白 / 非法 / 非有限 → null（与 yuanToCents 同口径）', () => {
    expect(yuanToPrice('')).toBeNull()
    expect(yuanToPrice('  ')).toBeNull()
    expect(yuanToPrice('1e3')).toBeNull()
    expect(yuanToPrice(NaN)).toBeNull()
    expect(yuanToPrice(Infinity)).toBeNull()
  })
})

describe('priceToYuan（万分之一元 → 元数值，表单回填）', () => {
  it('固定 ÷ 10000，不按币种小数位', () => {
    expect(priceToYuan(12345)).toBe(1.2345)
    expect(priceToYuan(150000)).toBe(15)
    expect(priceToYuan(0)).toBe(0)
  })
})
