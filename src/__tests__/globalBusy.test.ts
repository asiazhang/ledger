import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mockInvoke, type AppInvokeHandler } from './helpers/invoke-mock'
import { api } from '@/api'
import { busyVisible, resetGlobalBusy } from '@/composables/globalBusy'
import { stubReferenceInvoke } from './helpers/reference-stubs'


/** 手动完结的延迟 Promise：控制 invoke 完结时机以构造阈值与并发竞态 */
function deferred<T = unknown>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

beforeEach(() => {
  vi.useFakeTimers()
  mockInvoke.mockReset()
  // 模块级单例状态防串扰（先例：resetToastSink）
  resetGlobalBusy()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('globalBusy 全局忙碌状态模块（issue #500，统一 invoke 封装收口）', () => {
  it('在途 IO 超过 300ms 阈值后忙碌条可见，计数归零即隐藏', async () => {
    const d = deferred<void>()
    mockInvoke.mockReturnValue(d.promise)
    const pending = api.listCurrencies()

    expect(busyVisible.value).toBe(false) // 调用即计数，但阈值内不可见
    await vi.advanceTimersByTimeAsync(299)
    expect(busyVisible.value).toBe(false) // 阈值内
    await vi.advanceTimersByTimeAsync(1)
    expect(busyVisible.value).toBe(true) // 超阈值

    d.resolve([])
    await pending
    expect(busyVisible.value).toBe(false) // 计数归零即隐藏
  })

  it('阈值内结束的快操作从不显示忙碌条', async () => {
    mockInvoke.mockResolvedValue([])
    await api.listCurrencies()
    await vi.advanceTimersByTimeAsync(1000)
    expect(busyVisible.value).toBe(false)
  })

  it('并发调用聚合计数：部分完成仍显示，全部完成才隐藏', async () => {
    const a = deferred<void>()
    const b = deferred<void>()
    // 参考命令桩统一走共享助手（issue #725）：在途/兑结时机由覆写控制
    stubReferenceInvoke({
      list_currencies: () => a.promise,
      list_accounts: () => b.promise,
    })
    const p1 = api.listCurrencies()
    const p2 = api.listAccounts()

    await vi.advanceTimersByTimeAsync(300)
    expect(busyVisible.value).toBe(true)

    a.resolve([])
    await p1
    expect(busyVisible.value).toBe(true) // b 仍在途，聚合窗口未归零

    b.resolve([])
    await p2
    expect(busyVisible.value).toBe(false)
  })

  it('reject 路径正常递减：条不卡死，错误契约不变照常上抛', async () => {
    const d = deferred<void>()
    mockInvoke.mockReturnValue(d.promise)
    const pending = api.listCurrencies()

    await vi.advanceTimersByTimeAsync(300)
    expect(busyVisible.value).toBe(true)

    // Tauri invoke 失败形态：后端 AppError 的 serde 序列化对象（非 Error 实例）
    d.reject({ kind: 'internal', message: '炸了' })
    await expect(pending).rejects.toEqual({ kind: 'internal', message: '炸了' })
    expect(busyVisible.value).toBe(false)

    // 不卡死：后续慢 IO 仍能再次点亮忙碌条
    const d2 = deferred<void>()
    mockInvoke.mockReturnValue(d2.promise)
    const pending2 = api.listCurrencies()
    await vi.advanceTimersByTimeAsync(300)
    expect(busyVisible.value).toBe(true)
    d2.resolve([])
    await pending2
    expect(busyVisible.value).toBe(false)
  })

  it('invoke 替身返回裸值（非 thenable）也纳入聚合计数（invokeHandler 裸值处理器先例）', async () => {
    // 本用例刻意违反 invoke「恒返回 Promise」契约（invokeHandler 的函数型 handler
    // 裸返回值原样透传，见 factories.ts），验证忙碌计数对非 thenable 返回同样收敛；
    // 故仅在此单点豁免桩类型，不放宽 AppInvokeHandler 全局契约。
    mockInvoke.mockImplementation((() => []) as unknown as AppInvokeHandler)
    await api.listCurrencies()
    await vi.advanceTimersByTimeAsync(1000)
    expect(busyVisible.value).toBe(false) // 递减走兑底包装，归零彻底

    // 计数对称未被破坏：后续慢 IO 仍能正常点亮与隐藏
    const d = deferred<void>()
    mockInvoke.mockReturnValue(d.promise)
    const pending = api.listAccounts()
    await vi.advanceTimersByTimeAsync(300)
    expect(busyVisible.value).toBe(true)
    d.resolve([])
    await pending
    expect(busyVisible.value).toBe(false)
  })

  it('重叠在途窗口聚合：快调用与慢调用重叠使聚合窗口持续在途，跨阈值即点亮', async () => {
    const slow = deferred<void>()
    // 参考命令桩统一走共享助手（issue #725）：慢调用挂起、快调用即刻兑结
    stubReferenceInvoke({
      list_currencies: () => slow.promise,
      list_accounts: [],
    })
    const pSlow = api.listCurrencies()

    // t=100ms 起一个即刻完成的快调用；聚合窗口自 t=0 起持续在途不归零
    await vi.advanceTimersByTimeAsync(100)
    const pFast = api.listAccounts()
    await pFast
    expect(busyVisible.value).toBe(false) // 仍未越过阈值

    await vi.advanceTimersByTimeAsync(200) // t=300ms：聚合窗口持续在途越过阈值
    expect(busyVisible.value).toBe(true)

    slow.resolve([])
    await pSlow
    expect(busyVisible.value).toBe(false)
  })
})
