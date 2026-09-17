import { describe, it, expect } from 'vitest'
import { createLatestWinsGuard } from '@/composables/latest-wins'

describe('createLatestWinsGuard（最新胜出竞态纪元，issue #1401）', () => {
  it('开启新纪元使旧纪元过期：仅最新 start 的纪元 isCurrent 为真', () => {
    const guard = createLatestWinsGuard()
    const first = guard.start()
    expect(guard.isCurrent(first)).toBe(true)

    const second = guard.start()
    expect(guard.isCurrent(first)).toBe(false)
    expect(guard.isCurrent(second)).toBe(true)

    // 多次推进只认最后一个纪元
    const third = guard.start()
    expect(guard.isCurrent(second)).toBe(false)
    expect(guard.isCurrent(third)).toBe(true)
  })
})
