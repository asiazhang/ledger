import { describe, expect, it } from 'vitest'
import {
  isSubsequence,
  matchLabel,
  pinyinFilter,
  pinyinInitials,
} from '@/utils/pinyin-filter'
import type { SelectOption } from 'naive-ui'

describe('pinyinInitials', () => {
  it('中文字取拼音首字母（多音字按词组消歧）', () => {
    expect(pinyinInitials('招商银行')).toBe('zsyh')
    expect(pinyinInitials('银行')).toBe('yh')
    expect(pinyinInitials('万科物业')).toBe('wkwy')
  })

  it('ASCII 字母/数字小写保留，标点与空格跳过', () => {
    expect(pinyinInitials('ABC银行')).toBe('abcyh')
    expect(pinyinInitials('123')).toBe('123')
    expect(pinyinInitials('招商·银行 1')).toBe('zsyh1')
  })

  it('空字符串返回空', () => {
    expect(pinyinInitials('')).toBe('')
  })
})

describe('isSubsequence', () => {
  it('按序允许跳字命中', () => {
    expect(isSubsequence('wy', 'wkwy')).toBe(true)
    // 输入 `wy` 能命中首字母串为 `wxy`/`awya` 的选项（跳字命中）
    expect(isSubsequence('wy', 'wxy')).toBe(true)
    expect(isSubsequence('wy', 'awya')).toBe(true)
    // target 中不存在的字符无法命中
    expect(isSubsequence('wxy', 'wkwy')).toBe(false)
    expect(isSubsequence('awya', 'wkwy')).toBe(false)
  })

  it('大小写不敏感', () => {
    expect(isSubsequence('WY', 'wkwy')).toBe(true)
    expect(isSubsequence('wy', 'WKWY')).toBe(true)
  })

  it('空 pattern 恒命中，空 target 仅空 pattern 命中', () => {
    expect(isSubsequence('', 'wkwy')).toBe(true)
    expect(isSubsequence('', '')).toBe(true)
    expect(isSubsequence('w', '')).toBe(false)
  })

  it('重复字符需按序逐一消费', () => {
    expect(isSubsequence('ww', 'wkwy')).toBe(true)
    expect(isSubsequence('ww', 'wy')).toBe(false)
  })
})

describe('matchLabel（统一语义：原文连续子串 ∨ 首字母子序列，词条 AND）', () => {
  it('汉字连续子串命中，非连续不命中', () => {
    expect(matchLabel('支付', '支付宝')).toBe(true)
    expect(matchLabel('付宝', '支付宝')).toBe(true)
    expect(matchLabel('支宝', '支付宝')).toBe(false)
  })

  it('拼音首字母子序列命中', () => {
    expect(matchLabel('zsyh', '招商银行')).toBe(true)
    expect(matchLabel('wy', '万科物业')).toBe(true)
    expect(matchLabel('wy', '万科小区物业')).toBe(true)
    expect(matchLabel('yh', '招商银行')).toBe(true)
    expect(matchLabel('zs', '招商银行')).toBe(true)
  })

  it('首字母不构成子序列则不命中', () => {
    expect(matchLabel('ws', '万科物业')).toBe(false)
    expect(matchLabel('yhz', '招商银行')).toBe(false)
  })

  it('大小写不敏感', () => {
    expect(matchLabel('ZSYH', '招商银行')).toBe(true)
    expect(matchLabel('Wy', '万科物业')).toBe(true)
    expect(matchLabel('cny', 'CNY现金')).toBe(true)
    expect(matchLabel('CNY', 'cny现金')).toBe(true)
  })

  it('混合输入「招zsyh」：含汉字词条对纯 ASCII 首字母串必然失败，落原文子串路径', () => {
    expect(matchLabel('招zsyh', '招商银行')).toBe(false)
    expect(matchLabel('招', '招商银行')).toBe(true)
    expect(matchLabel('招行', '招商银行')).toBe(false)
  })

  it('纯 ASCII 名称按原文子串或首字母保留命中', () => {
    expect(matchLabel('abcyh', 'ABC银行')).toBe(true)
    expect(matchLabel('abc', 'ABC银行')).toBe(true)
    expect(matchLabel('123', '零钱123')).toBe(true)
  })

  it('空输入恒命中（恢复完整列表）', () => {
    expect(matchLabel('', '招商银行')).toBe(true)
    expect(matchLabel('   ', '招商银行')).toBe(true)
  })

  it('多词条之间 AND', () => {
    expect(matchLabel('w k', '万科物业')).toBe(true)
    expect(matchLabel('wy 现金', '万科物业')).toBe(false)
    expect(matchLabel('zsyh 招', '招商银行')).toBe(true)
  })
})

describe('pinyinFilter（NSelect filter 签名收口）', () => {
  const option = (label: string): SelectOption =>
    ({ label, value: label }) as SelectOption

  it('按 option.label 判定', () => {
    expect(pinyinFilter('zsyh', option('招商银行'))).toBe(true)
    expect(pinyinFilter('ws', option('万科物业'))).toBe(false)
  })

  it('数字 label 转字符串判定', () => {
    expect(pinyinFilter('12', { label: 123, value: 123 } as SelectOption)).toBe(true)
  })

  it('非字符串/数字 label（渲染函数）不参与拼音判定，恒显示', () => {
    const opt = { label: () => 'x', value: 1 } as unknown as SelectOption
    expect(pinyinFilter('zsyh', opt)).toBe(true)
  })
})
