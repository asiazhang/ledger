// storage 工具测试：JSON 序列化 + 静默容错（loadLocal / saveLocal / removeLocal）。
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { loadLocal, saveLocal, removeLocal } from '@/utils/storage'

beforeEach(() => {
  localStorage.clear()
})

describe('loadLocal / saveLocal', () => {
  it('saveLocal JSON 序列化写入，loadLocal 读回', () => {
    saveLocal('k', { a: 1 })
    expect(localStorage.getItem('k')).toBe('{"a":1}')
    expect(loadLocal('k', null)).toEqual({ a: 1 })
  })

  it('无记录时回退 fallback', () => {
    expect(loadLocal('missing', 'fallback')).toBe('fallback')
  })

  it('损坏的 JSON 静默回退 fallback', () => {
    localStorage.setItem('bad', 'not-json{')
    expect(loadLocal('bad', 'fallback')).toBe('fallback')
  })

  it('setItem 抛错（配额等）时静默容错', () => {
    const spy = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('quota exceeded')
    })
    expect(() => saveLocal('k', 1)).not.toThrow()
    spy.mockRestore()
  })
})

describe('removeLocal（issue #270）', () => {
  it('移除已存 key 后 loadLocal 回退 fallback', () => {
    saveLocal('k', 'v')
    removeLocal('k')
    expect(localStorage.getItem('k')).toBeNull()
    expect(loadLocal('k', 'fallback')).toBe('fallback')
  })

  it('移除不存在的 key 不报错', () => {
    expect(() => removeLocal('missing')).not.toThrow()
  })

  it('removeItem 抛错时静默容错', () => {
    const spy = vi.spyOn(Storage.prototype, 'removeItem').mockImplementation(() => {
      throw new Error('storage error')
    })
    expect(() => removeLocal('k')).not.toThrow()
    spy.mockRestore()
  })
})
