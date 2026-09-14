import { describe, it, expect } from 'vitest'
import { formatRate } from '../index'

describe('formatRate 收益率展示口径单点（issue #1195 / ADR-0115）', () => {
  it('正收益率带正号、保留两位小数', () => {
    expect(formatRate(0.1)).toBe('+10.00%')
    expect(formatRate(0.123456)).toBe('+12.35%')
  })

  it('负收益率带负号，零不带符号', () => {
    expect(formatRate(-0.05)).toBe('-5.00%')
    expect(formatRate(0)).toBe('0.00%')
  })

  it('跟随界面语言（zh-CN 与 en-US 同为百分数刻度）', () => {
    expect(formatRate(0.1, 'zh-CN')).toBe('+10.00%')
    expect(formatRate(0.1, 'en-US')).toBe('+10.00%')
  })
})
