import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { defineComponent, h } from 'vue'
import { mount, flushPromises } from '@vue/test-utils'
import { listen } from '@tauri-apps/api/event'
import { useInstrumentFullSync } from '@/composables/useInstrumentFullSync'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import type { SyncProgress } from '@/types'

const mockListen = vi.mocked(listen)

let capturedHandler: ((event: { payload: SyncProgress }) => void) | undefined

beforeEach(() => {
  mockInvoke.mockReset()
  mockListen.mockReset()
  capturedHandler = undefined
  mockListen.mockImplementation((_event, handler) => {
    capturedHandler = handler as (event: { payload: SyncProgress }) => void
    return Promise.resolve(() => {})
  })
})

type SyncApi = ReturnType<typeof useInstrumentFullSync>

function mountHost(): SyncApi {
  let sync!: SyncApi
  mount(
    defineComponent({
      setup() {
        sync = useInstrumentFullSync()
        return () => h('div')
      },
    }),
  )
  return sync
}

async function mountHostReady(): Promise<SyncApi> {
  const sync = mountHost()
  await flushPromises()
  return sync
}

function emitProgress(p: Partial<SyncProgress>) {
  capturedHandler?.({
    payload: {
      current: 0,
      total: 0,
      market: '',
      done: false,
      total_inserted: 0,
      total_updated: 0,
      error: null,
      cancelled: false,
      ...p,
    },
  })
}

describe('useInstrumentFullSync 全量同步接缝（issue #109）', () => {
  it('仅打开确认框不会调用 sync_instruments（未确认不发起同步）', async () => {
    mockInvoke.mockRejectedValue(new Error('不应被调用'))
    const sync = await mountHostReady()
    sync.openConfirm()
    expect(sync.confirmOpen.value).toBe(true)
    expect(mockInvoke).not.toHaveBeenCalledWith('sync_instruments')
  })

  it('closeConfirm 关闭确认框，confirmSync 后确认框关闭', async () => {
    mockInvoke.mockResolvedValue(undefined)
    const sync = await mountHostReady()
    sync.openConfirm()
    expect(sync.confirmOpen.value).toBe(true)
    sync.closeConfirm()
    expect(sync.confirmOpen.value).toBe(false)
    // confirmSync 触发后确认框应关闭且发起同步
    sync.openConfirm()
    await sync.confirmSync()
    expect(sync.confirmOpen.value).toBe(false)
    expect(mockInvoke).toHaveBeenCalledWith('sync_instruments')
  })

  it('确认（confirmSync）后调用 sync_instruments 并置 syncing、打开进度框', async () => {
    mockInvoke.mockResolvedValue(undefined)
    const sync = await mountHostReady()
    await sync.confirmSync()
    expect(mockInvoke).toHaveBeenCalledWith('sync_instruments')
    expect(sync.syncStatus.value).toBe('syncing')
    expect(sync.modalOpen.value).toBe(true)
  })

  it('进度事件更新 current/total/inserted/updated/progress', async () => {
    mockInvoke.mockResolvedValue(undefined)
    const sync = await mountHostReady()
    await sync.startSync()
    emitProgress({ current: 120, total: 300, total_inserted: 5, total_updated: 7 })
    expect(sync.current.value).toBe(120)
    expect(sync.total.value).toBe(300)
    expect(sync.inserted.value).toBe(5)
    expect(sync.updated.value).toBe(7)
    expect(sync.progress.value).toBe(40)
  })

  it('完成终态（done + cancelled=false）置状态为 done，展示新增/更新', async () => {
    mockInvoke.mockResolvedValue(undefined)
    const sync = await mountHostReady()
    await sync.startSync()
    emitProgress({ done: true, cancelled: false, total_inserted: 10, total_updated: 4 })
    expect(sync.syncStatus.value).toBe('done')
    expect(sync.inserted.value).toBe(10)
    expect(sync.updated.value).toBe(4)
  })

  it('中断（done + cancelled=true）置状态为 cancelled，展示已同步计数', async () => {
    stubReferenceInvoke({
      sync_instruments: () => Promise.resolve(undefined),
      cancel_sync_instruments: () =>
        Promise.resolve({ cancelled: true, message: '已请求中断同步' }),
    })
    const sync = await mountHostReady()
    await sync.startSync()
    expect(sync.syncStatus.value).toBe('syncing')
    await sync.requestCancel()
    expect(mockInvoke).toHaveBeenCalledWith('cancel_sync_instruments')
    emitProgress({ done: true, cancelled: true, total_inserted: 3, total_updated: 2 })
    expect(sync.syncStatus.value).toBe('cancelled')
    expect(sync.inserted.value).toBe(3)
    expect(sync.updated.value).toBe(2)
  })

  it('失败终态（error）置状态为 error 并携带错误信息', async () => {
    mockInvoke.mockResolvedValue(undefined)
    const sync = await mountHostReady()
    await sync.startSync()
    emitProgress({ done: true, error: '请求被限流' })
    expect(sync.syncStatus.value).toBe('error')
    expect(sync.errorMessage.value).toBe('请求被限流')
  })

  it('关闭进度框不影响同步状态（后台继续），可重开', async () => {
    mockInvoke.mockResolvedValue(undefined)
    const sync = await mountHostReady()
    await sync.startSync()
    expect(sync.modalOpen.value).toBe(true)
    sync.closeModal()
    expect(sync.modalOpen.value).toBe(false)
    expect(sync.syncStatus.value).toBe('syncing')
    // 重开仍可见进度
    sync.openModal()
    expect(sync.modalOpen.value).toBe(true)
    expect(sync.syncStatus.value).toBe('syncing')
  })

  it('同步进行中再次 startSync 被守卫短路（不重复 invoke）', async () => {
    let resolveSync!: (v: unknown) => void
    mockInvoke.mockImplementation(() => new Promise((res) => { resolveSync = res }))
    const sync = await mountHostReady()
    const p1 = sync.startSync()
    expect(sync.syncStatus.value).toBe('syncing')
    // 进行中再次启动：被短路，不新增 invoke
    await sync.startSync()
    expect(sync.syncStatus.value).toBe('syncing')
    resolveSync(undefined)
    await p1
    expect(mockInvoke).toHaveBeenCalledTimes(1)
  })
})
