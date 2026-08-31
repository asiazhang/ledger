import { describe, it, expect, vi, beforeEach } from 'vitest'
import { useLoadable, registerToastSink } from '@/composables/useLoadable'
import { makeFakeSink, resetToastSink } from './factories'

/** 手动完结的延迟 Promise：控制任务完结时机以构造竞态 */
function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

beforeEach(() => {
  // 每用例复位为 no-op，模拟「注册前」默认态，防模块级 sink 状态串扰
  resetToastSink()
})

describe('useLoadable 异步任务模块（ADR-0040 / issue #320）', () => {
  it('发起后 loading 置位，成功后收尾；成功回结果，error 保持空', async () => {
    const task = vi.fn(async () => '结果')
    const { loading, error, run } = useLoadable(task)

    expect(loading.value).toBe(false)

    const pending = run()
    expect(loading.value).toBe(true)

    await expect(pending).resolves.toBe('结果')
    expect(loading.value).toBe(false)
    expect(error.value).toBeNull()
    expect(task).toHaveBeenCalledTimes(1)
  })

  it('发起永不 reject：失败回空、error 置位（文案经统一归一取 message 字段）', async () => {
    const { error, run } = useLoadable(async () => {
      // Tauri invoke 失败形态：后端 AppError 的 serde 序列化对象（非 Error 实例）
      throw { kind: 'missing_rate', message: '缺少 USD→CNY 汇率，无法折算' }
    })

    await expect(run()).resolves.toBeNull()
    expect(error.value).toBe('缺少 USD→CNY 汇率，无法折算')
  })

  it('任务同步抛错同样走失败通道，发起 promise 不 reject', async () => {
    const { error, run } = useLoadable(() => {
      throw new Error('同步炸了')
    })

    await expect(run()).resolves.toBeNull()
    expect(error.value).toBe('同步炸了')
  })

  it('默认策略 = 统一 toast：失败时经 sink 弹出归一文案；error 状态与 toast 双通道共存', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)

    const failing = useLoadable(async () => Promise.reject(new Error('网络开小差')))
    await failing.run()

    expect(sink.error).toHaveBeenCalledTimes(1)
    expect(sink.error).toHaveBeenCalledWith('网络开小差')
    expect(failing.error.value).toBe('网络开小差')

    sink.error.mockClear()
    const ok = useLoadable(async () => '成功')
    await ok.run()
    expect(sink.error).not.toHaveBeenCalled()
  })

  it('注册前 sink 为 no-op：失败不炸，发起照常回空且 error 置位', async () => {
    // beforeEach 已复位 no-op sink，此处刻意不再注册假 sink
    const { error, run } = useLoadable(async () => Promise.reject('注册前的失败'))

    await expect(run()).resolves.toBeNull()
    expect(error.value).toBe('注册前的失败')
  })

  it('竞态：后发覆盖先发，先发迟到结果连同其 loading 收尾一并作废', async () => {
    const gates = [deferred<string>(), deferred<string>()]
    let n = 0
    const { loading, error, run } = useLoadable(() => gates[n++].promise)

    const p1 = run() // 用 gates[0]
    const p2 = run() // 用 gates[1]
    expect(loading.value).toBe(true)

    // 后发先至：终态 = 最后一次发起的结果
    gates[1].resolve('后发结果')
    await expect(p2).resolves.toBe('后发结果')
    expect(loading.value).toBe(false)
    expect(error.value).toBeNull()

    // 先发迟到：结果作废（回空），loading 收尾一并作废（不扰动已收尾的终态）
    gates[0].resolve('先发的迟到结果')
    await expect(p1).resolves.toBeNull()
    expect(loading.value).toBe(false)
    expect(error.value).toBeNull()
  })

  it('竞态：后发仍在飞行时，先发迟到的 loading 收尾不生效', async () => {
    const gates = [deferred<string>(), deferred<string>()]
    let n = 0
    const { loading, error, run } = useLoadable(() => gates[n++].promise)

    const p1 = run() // 用 gates[0]
    void run() // 用 gates[1]，保持飞行
    expect(loading.value).toBe(true)

    // 先发先至而后发未完：迟到结果作废，loading 不被其收尾
    gates[0].resolve('迟到的先发结果')
    await expect(p1).resolves.toBeNull()
    expect(loading.value).toBe(true)
    expect(error.value).toBeNull()

    // 后发完结才真正收 loading
    gates[1].resolve('后发结果')
    await vi.waitFor(() => expect(loading.value).toBe(false))
  })

  it('竞态：先发迟到失败一并作废——error 不置位、默认 toast 不弹', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    const gates = [deferred<string>(), deferred<string>()]
    let n = 0
    const { error, run } = useLoadable(() => gates[n++].promise)

    const p1 = run() // 用 gates[0]
    const p2 = run() // 用 gates[1]

    gates[1].resolve('后发成功')
    await expect(p2).resolves.toBe('后发成功')
    expect(error.value).toBeNull()

    gates[0].reject('先发的迟到失败')
    await expect(p1).resolves.toBeNull()
    expect(error.value).toBeNull()
    expect(sink.error).not.toHaveBeenCalled()
  })

  it('刷新即再次发起：同一方法重复触发拿到新结果', async () => {
    let n = 0
    const { loading, error, run } = useLoadable(async () => `第${++n}次`)

    await run()
    await expect(run()).resolves.toBe('第2次')
    expect(loading.value).toBe(false)
    expect(error.value).toBeNull()
  })

  it('失败后重试成功：error 清零（error 是唯一成败判据）', async () => {
    let fail = true
    const { error, run } = useLoadable(async () => {
      if (fail) throw new Error('第一次失败')
      return '好了'
    })

    await run()
    expect(error.value).toBe('第一次失败')

    fail = false
    await expect(run()).resolves.toBe('好了')
    expect(error.value).toBeNull()
  })
})
