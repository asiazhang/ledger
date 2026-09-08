import { describe, it, expect } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { useInstrumentInfoSync } from '@/composables/useInstrumentInfoSync'


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
