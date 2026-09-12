import { describe, it, expect, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { mockInvoke } from '@ledger/test-support/invoke-mock'
import {
  captureListenHandlers,
  mockListen,
  type CapturedListener,
} from '@ledger/test-support/listen-mock'
import {
  INSTRUMENT_SYNC_PROGRESS_EVENT,
  resetInstrumentInfoSyncForTest,
  useInstrumentInfoSync,
} from '@/composables/useInstrumentInfoSync'


describe('useInstrumentInfoSync 标的信息同步（标的页/盈亏页共用接缝）', () => {
  it('空库时同步：resolve success（success 子形态），message 为「暂无标的可同步」', async () => {
    mockInvoke.mockResolvedValue({
      synced: 0,
      skipped: 0,
      message: '暂无标的可同步',
    })
    const { syncing, status, resultMessage, lastResult, sync } = useInstrumentInfoSync()
    await expect(sync()).resolves.toBe('success')
    expect(syncing.value).toBe(false)
    expect(status.value).toBe('success')
    expect(resultMessage.value).toBe('暂无标的可同步')
    expect(lastResult.value).toMatchObject({ synced: 0, skipped: 0 })
  })

  it('有持仓时同步：resolve success，message 为同步/跳过统计', async () => {
    mockInvoke.mockResolvedValue({
      synced: 2,
      skipped: 1,
      message: '已同步 2 只，跳过 1 只',
    })
    const { status, resultMessage, lastResult, sync } = useInstrumentInfoSync()
    await expect(sync()).resolves.toBe('success')
    expect(status.value).toBe('success')
    expect(resultMessage.value).toBe('已同步 2 只，跳过 1 只')
    expect(lastResult.value).toMatchObject({ synced: 2, skipped: 1 })
  })

  it('同步失败：resolve error，status 为 error，message 携带具体错误原因', async () => {
    mockInvoke.mockRejectedValue(new Error('网络错误'))
    const { status, resultMessage, sync } = useInstrumentInfoSync()
    await expect(sync()).resolves.toBe('error')
    expect(status.value).toBe('error')
    expect(resultMessage.value).toBe('同步失败：网络错误')
  })

  it('同步进行中再次调用 sync 被短路：复用在途承诺，终态一致且只触发一次 invoke', async () => {
    let resolveSync!: (v: unknown) => void
    mockInvoke.mockImplementation(
      () => new Promise((res) => { resolveSync = res }),
    )
    const { syncing, sync } = useInstrumentInfoSync()
    const p1 = sync()
    expect(syncing.value).toBe(true)
    // 进行中再次调用：应被短路（复用在途承诺），不新增 invoke
    const p2 = sync()
    expect(mockInvoke).toHaveBeenCalledTimes(1)
    resolveSync({ synced: 1, skipped: 0, message: '已同步 1 只，跳过 0 只' })
    await expect(p1).resolves.toBe('success')
    await expect(p2).resolves.toBe('success')
    expect(syncing.value).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// 同步确定进度（issue #897 / ADR-0095）：接缝订阅后端进度事件、产出可观察
// 进度、终态（成功/失败）收起清空；在途短路期间进度共享。
// ---------------------------------------------------------------------------

let progressHandlers: CapturedListener[] = []

beforeEach(() => {
  resetInstrumentInfoSyncForTest()
  progressHandlers = captureListenHandlers()
})

/** 触发最近捕获的进度事件监听器（tauri Event 载荷形状）。 */
function fireProgress(payload: unknown): void {
  expect(progressHandlers.length).toBeGreaterThan(0)
  progressHandlers.at(-1)!( { event: INSTRUMENT_SYNC_PROGRESS_EVENT, payload })
}

describe('useInstrumentInfoSync 同步进度（issue #897）', () => {
  it('订阅后端进度事件 ledger:instrument-sync-progress（事件名常量单点）', () => {
    useInstrumentInfoSync()
    expect(mockListen).toHaveBeenCalledWith(
      INSTRUMENT_SYNC_PROGRESS_EVENT,
      expect.any(Function),
    )
    expect(INSTRUMENT_SYNC_PROGRESS_EVENT).toBe('ledger:instrument-sync-progress')
  })

  it('在途同步期间消费进度事件，产出可观察进度', async () => {
    let resolveSync!: (v: unknown) => void
    mockInvoke.mockImplementation(
      () => new Promise((res) => { resolveSync = res }),
    )
    const { progress, sync } = useInstrumentInfoSync()
    const p = sync()
    expect(progress.value).toBeNull()

    fireProgress({ done: 0, total: 100 })
    await flushPromises()
    expect(progress.value).toEqual({ done: 0, total: 100 })

    fireProgress({ done: 37, total: 100 })
    await flushPromises()
    expect(progress.value).toEqual({ done: 37, total: 100 })

    resolveSync({ synced: 100, skipped: 0, message: '已同步 100 只，跳过 0 只' })
    await expect(p).resolves.toBe('success')
  })

  it('同步成功终态收起清空进度', async () => {
    let resolveSync!: (v: unknown) => void
    mockInvoke.mockImplementation(
      () => new Promise((res) => { resolveSync = res }),
    )
    const { progress, sync } = useInstrumentInfoSync()
    const p = sync()
    fireProgress({ done: 50, total: 100 })
    await flushPromises()
    expect(progress.value).toEqual({ done: 50, total: 100 })

    resolveSync({ synced: 100, skipped: 0, message: '已同步 100 只，跳过 0 只' })
    await expect(p).resolves.toBe('success')
    expect(progress.value).toBeNull()

    // 终态之后迟到的进度事件不再复活进度条
    fireProgress({ done: 99, total: 100 })
    await flushPromises()
    expect(progress.value).toBeNull()
  })

  it('同步失败终态同样收起清空进度', async () => {
    let rejectSync!: (e: unknown) => void
    mockInvoke.mockImplementation(
      () => new Promise((_, rej) => { rejectSync = rej }),
    )
    const { progress, status, sync } = useInstrumentInfoSync()
    const p = sync()
    fireProgress({ done: 12, total: 100 })
    await flushPromises()
    expect(progress.value).toEqual({ done: 12, total: 100 })

    rejectSync(new Error('网络错误'))
    await expect(p).resolves.toBe('error')
    expect(status.value).toBe('error')
    expect(progress.value).toBeNull()
  })

  it('无在途同步时进度事件被忽略（守卫：事件只在同步进行中才有意义）', async () => {
    const { progress } = useInstrumentInfoSync()
    fireProgress({ done: 42, total: 100 })
    await flushPromises()
    expect(progress.value).toBeNull()
  })

  it('新一次同步开始时重置进度（上次的进度不残留）', async () => {
    let resolveSync!: (v: unknown) => void
    mockInvoke.mockImplementation(
      () => new Promise((res) => { resolveSync = res }),
    )
    const { progress, sync } = useInstrumentInfoSync()
    const p1 = sync()
    fireProgress({ done: 80, total: 100 })
    await flushPromises()
    resolveSync({ synced: 100, skipped: 0, message: '已同步 100 只，跳过 0 只' })
    await expect(p1).resolves.toBe('success')

    // 重新发起：进度从零开始，等下一次事件
    let resolveSync2!: (v: unknown) => void
    mockInvoke.mockImplementation(
      () => new Promise((res) => { resolveSync2 = res }),
    )
    const p2 = sync()
    expect(progress.value).toBeNull()
    fireProgress({ done: 1, total: 100 })
    await flushPromises()
    expect(progress.value).toEqual({ done: 1, total: 100 })
    resolveSync2({ synced: 100, skipped: 0, message: '已同步 100 只，跳过 0 只' })
    await expect(p2).resolves.toBe('success')
  })

  it('在途短路期间进度共享：另一实例消费同一份进度状态，终态反馈零分叉', async () => {
    let resolveSync!: (v: unknown) => void
    mockInvoke.mockImplementation(
      () => new Promise((res) => { resolveSync = res }),
    )
    const first = useInstrumentInfoSync()
    const p1 = first.sync()

    // 另一入口（另一实例）创建后消费同一份进度；重复 sync 短路复用在途承诺
    const second = useInstrumentInfoSync()
    const p2 = second.sync()
    expect(mockInvoke).toHaveBeenCalledTimes(1)

    fireProgress({ done: 60, total: 100 })
    await flushPromises()
    expect(second.progress.value).toEqual({ done: 60, total: 100 })
    expect(first.progress.value).toEqual({ done: 60, total: 100 })

    resolveSync({ synced: 100, skipped: 0, message: '已同步 100 只，跳过 0 只' })
    await expect(p1).resolves.toBe('success')
    await expect(p2).resolves.toBe('success')
    expect(first.progress.value).toBeNull()
    expect(second.progress.value).toBeNull()
    // 终态反馈两入口同显同一份（短路口不缺按钮 loading 与结果消息）
    expect(second.resultMessage.value).toBe('已同步 100 只，跳过 0 只')
    expect(second.status.value).toBe('success')
  })

  it('短路入口的失败终态反馈同样到达（错误消息零分叉）', async () => {
    let rejectSync!: (e: unknown) => void
    mockInvoke.mockImplementation(
      () => new Promise((_, rej) => { rejectSync = rej }),
    )
    const first = useInstrumentInfoSync()
    const p1 = first.sync()
    const second = useInstrumentInfoSync()
    const p2 = second.sync()

    rejectSync(new Error('网络错误'))
    await expect(p1).resolves.toBe('error')
    await expect(p2).resolves.toBe('error')
    expect(second.resultMessage.value).toBe('同步失败：网络错误')
    expect(second.status.value).toBe('error')
    expect(second.progress.value).toBeNull()
  })

  it('载荷形状异常的进度事件被忽略（防脏 payload 渲染）', async () => {
    let resolveSync!: (v: unknown) => void
    mockInvoke.mockImplementation(
      () => new Promise((res) => { resolveSync = res }),
    )
    const { progress, sync } = useInstrumentInfoSync()
    const p = sync()
    fireProgress(undefined)
    fireProgress({ done: 'x' })
    fireProgress({ done: Number.NaN, total: Number.NaN })
    await flushPromises()
    expect(progress.value).toBeNull()
    resolveSync({ synced: 0, skipped: 0, message: '暂无标的可同步' })
    await expect(p).resolves.toBe('success')
  })

  it('基金深回填的页级明细随进度一并产出，标的级推进清除明细（issue #1061）', async () => {
    // 首刷回填单只基金期间：done/total 停在标的级口径，页明细持续推进；
    // 基金完成后的标的级推进（无 fund 字段）把页明细清掉。
    let resolveSync!: (v: unknown) => void
    mockInvoke.mockImplementation(
      () => new Promise((res) => { resolveSync = res }),
    )
    const { progress, sync } = useInstrumentInfoSync()
    const p = sync()

    fireProgress({ done: 0, total: 1, fund: { code: '110022', page: 3, pages: 25 } })
    await flushPromises()
    expect(progress.value).toEqual({
      done: 0,
      total: 1,
      fund: { code: '110022', page: 3, pages: 25 },
    })

    fireProgress({ done: 1, total: 1 })
    await flushPromises()
    expect(progress.value).toEqual({ done: 1, total: 1 })
    expect(progress.value?.fund).toBeUndefined()

    resolveSync({ synced: 1, skipped: 0, message: '已同步 1 只，跳过 0 只' })
    await expect(p).resolves.toBe('success')
  })

  it('页级明细形状异常时丢弃明细、保留标的级进度（防脏 payload 渲染）', async () => {
    let resolveSync!: (v: unknown) => void
    mockInvoke.mockImplementation(
      () => new Promise((res) => { resolveSync = res }),
    )
    const { progress, sync } = useInstrumentInfoSync()
    const p = sync()
    fireProgress({ done: 2, total: 3, fund: { code: 42, page: 'x', pages: 0 } })
    await flushPromises()
    expect(progress.value).toEqual({ done: 2, total: 3 })
    resolveSync({ synced: 3, skipped: 0, message: '已同步 3 只，跳过 0 只' })
    await expect(p).resolves.toBe('success')
  })
})
