import { describe, it, expect, vi, beforeEach } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { useHoldingPriceSync } from '@/composables/useHoldingPriceSync'

const mockInvoke = vi.mocked(invoke)

beforeEach(() => {
  mockInvoke.mockReset()
})

describe('useHoldingPriceSync 持仓价格增量同步（标的页/盈亏页共用接缝）', () => {
  it('无持仓时同步：message 为「无持仓标的可同步」，状态为 success（不视为错误）', async () => {
    mockInvoke.mockResolvedValue({
      synced: 0,
      skipped: 0,
      message: '无持仓标的可同步',
    })
    const { syncing, status, resultMessage, lastResult, sync } = useHoldingPriceSync()
    await sync()
    expect(syncing.value).toBe(false)
    expect(status.value).toBe('success')
    expect(resultMessage.value).toBe('无持仓标的可同步')
    expect(lastResult.value).toMatchObject({ synced: 0, skipped: 0 })
  })

  it('有持仓时同步：message 为同步/跳过统计，状态为 success', async () => {
    mockInvoke.mockResolvedValue({
      synced: 2,
      skipped: 1,
      message: '已同步 2 只，跳过 1 只',
    })
    const { status, resultMessage, lastResult, sync } = useHoldingPriceSync()
    await sync()
    expect(status.value).toBe('success')
    expect(resultMessage.value).toBe('已同步 2 只，跳过 1 只')
    expect(lastResult.value).toMatchObject({ synced: 2, skipped: 1 })
  })

  it('同步失败：status 为 error，message 携带具体错误原因', async () => {
    mockInvoke.mockRejectedValue(new Error('网络错误'))
    const { status, resultMessage, sync } = useHoldingPriceSync()
    await sync()
    expect(status.value).toBe('error')
    expect(resultMessage.value).toBe('同步失败：网络错误')
  })

  it('同步进行中再次调用 sync 被短路（只触发一次 invoke）', async () => {
    let resolveSync!: (v: unknown) => void
    mockInvoke.mockImplementation(
      () => new Promise((res) => { resolveSync = res }),
    )
    const { syncing, sync } = useHoldingPriceSync()
    const p1 = sync()
    expect(syncing.value).toBe(true)
    // 进行中再次调用：应被短路，不新增 invoke
    await sync()
    expect(mockInvoke).toHaveBeenCalledTimes(1)
    resolveSync({ synced: 1, skipped: 0, message: '已同步 1 只，跳过 0 只' })
    await p1
    expect(syncing.value).toBe(false)
  })
})
