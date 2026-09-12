import { describe, it, expect, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { withSetup } from '@ledger/test-support/mount'
import { useReferenceStore } from '@/stores/reference'
import { useDashboardOverview } from '@/composables/useDashboardOverview'
import { registerToastSink } from '@/composables/useLoadable'
import { makeFakeSink, makeOverview, resetToastSink } from './factories'


const mockOverview = makeOverview({ net_worth_cents: 1234567, accounts_balance_cents: 1000000 })

/** 默认 invoke 布线：净资产总览契约快照（参考字典命令走接缝内建兜底，不在此枚举） */
const BASE_DEFAULTS = { dashboard_overview: mockOverview }

beforeEach(async () => {
  wireInvokeSeam({ defaults: BASE_DEFAULTS })
  // 每用例复位为 no-op，模拟「注册前」默认态，防模块级 sink 状态串扰
  resetToastSink()
  const store = useReferenceStore()
  await store.refresh()
})

describe('useDashboardOverview 首页净资产数据层（issue #143）', () => {
  it('加载 dashboard_overview 并装配出总览数据', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    const { overview, loading, error, refresh } = withSetup(() => useDashboardOverview())
    await refresh()
    expect(loading.value).toBe(false)
    expect(error.value).toBeNull()
    expect(overview.value).toEqual(mockOverview)
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'dashboard_overview')).toBe(true)
    // 成功不弹 toast
    expect(sink.error).not.toHaveBeenCalled()
  })

  it('命令报错（如缺汇率）时进入兜底状态：overview 置空、error 带后端中文错误信息，不抛异常', async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        dashboard_overview: () => Promise.reject(new Error('缺少 USD→CNY 汇率，无法折算')),
      },
    })
    const sink = makeFakeSink()
    registerToastSink(sink)
    // 挂载自动首刷吃到拒绝布线：治愈后不抛（未处理 rejection 会让测试失败）
    const { overview, loading, error } = withSetup(() => useDashboardOverview())
    await flushPromises()
    expect(loading.value).toBe(false)
    expect(overview.value).toBeNull()
    expect(error.value).toBe('缺少 USD→CNY 汇率，无法折算')
    // 失败弹默认 toast（归一文案），error 状态与 toast 双通道共存
    expect(sink.error).toHaveBeenCalledTimes(1)
    expect(sink.error).toHaveBeenCalledWith('缺少 USD→CNY 汇率，无法折算')
  })

  it('非 Error 抛出值（如 Tauri 字符串错误）也能兜底为文案', async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { dashboard_overview: () => Promise.reject('缺汇率') },
    })
    const { error, refresh } = withSetup(() => useDashboardOverview())
    await refresh()
    expect(error.value).toBe('缺汇率')
  })

  it('成功后再次报错：overview 清空并切换到错误态；再次成功则恢复', async () => {
    const { overview, error, refresh } = withSetup(() => useDashboardOverview())
    await refresh()
    expect(overview.value).not.toBeNull()

    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        dashboard_overview: () => Promise.reject(new Error('缺少 HKD→CNY 汇率')),
      },
    })
    await refresh()
    expect(overview.value).toBeNull()
    expect(error.value).toBe('缺少 HKD→CNY 汇率')

    wireInvokeSeam({ defaults: BASE_DEFAULTS })
    await refresh()
    expect(overview.value).toEqual(mockOverview)
    expect(error.value).toBeNull()
  })

  it('失败 toast 只在失败那次弹出：错误态↔成功态往返中 sink 各就各位', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        dashboard_overview: () => Promise.reject(new Error('缺少 HKD→CNY 汇率')),
      },
    })
    // 挂载自动首刷已吃到拒绝布线并弹一次 toast，无需再显式首刷
    const { refresh } = withSetup(() => useDashboardOverview())
    await flushPromises()
    expect(sink.error).toHaveBeenCalledTimes(1)

    wireInvokeSeam({ defaults: BASE_DEFAULTS })
    await refresh()
    expect(sink.error).toHaveBeenCalledTimes(1)
  })
})
