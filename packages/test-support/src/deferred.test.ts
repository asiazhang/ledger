import { describe, it, expect } from 'vitest'
import { deferred } from './deferred'

describe('deferred（手动完结替身，issue #1393）', () => {
  it('resolve 以给定值完结 promise', async () => {
    const d = deferred<string[]>()
    d.resolve(['a', 'b'])
    await expect(d.promise).resolves.toEqual(['a', 'b'])
  })

  it('reject 以给定理由拒绝 promise', async () => {
    const d = deferred<string[]>()
    d.reject(new Error('boom'))
    await expect(d.promise).rejects.toThrow('boom')
  })

  it('未完结时 promise 保持在途（不自动落定）', async () => {
    const d = deferred<void>()
    let settled = false
    void d.promise.then(() => {
      settled = true
    })
    await Promise.resolve()
    expect(settled).toBe(false)
    d.resolve()
    await d.promise
    expect(settled).toBe(true)
  })
})
