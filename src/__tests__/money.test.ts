import { describe, it, expect } from 'vitest'
import { yuanToCents } from '@/utils/money'

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
