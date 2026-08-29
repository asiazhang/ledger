import { describe, expect, it } from 'vitest'
import { errorMessage } from '@/utils/errors'

describe('errorMessage', () => {
  it('从 Tauri 后端 AppError 序列化形态中提取 message', () => {
    expect(errorMessage({ kind: 'Invalid', message: '该分类已存在按月预算' })).toBe(
      '该分类已存在按月预算',
    )
  })

  it('message 非字符串或缺失时回退 String()', () => {
    expect(errorMessage({ kind: 'Db' })).toBe('[object Object]')
    expect(errorMessage({ message: 42 })).toBe('[object Object]')
  })

  it('字符串原样返回', () => {
    expect(errorMessage('预算金额必须为正数')).toBe('预算金额必须为正数')
  })

  it('Error 实例返回其 message', () => {
    expect(errorMessage(new Error('网络错误'))).toBe('网络错误')
  })

  it('null/undefined 走 String() 兜底', () => {
    expect(errorMessage(null)).toBe('null')
    expect(errorMessage(undefined)).toBe('undefined')
  })
})
