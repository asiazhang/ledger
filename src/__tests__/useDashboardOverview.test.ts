import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import { useDashboardOverview } from '@/composables/useDashboardOverview'
import { registerToastSink } from '@/composables/useLoadable'
import {
  invokeHandler,
  makeFakeSink,
  makeOverview,
  mockCurrencies,
  resetToastSink,
} from './factories'

const mockInvoke = vi.mocked(invoke)

const mockOverview = makeOverview({ net_worth_cents: 1234567, accounts_balance_cents: 1000000 })

/** 默认 invoke mock：参考数据 + 净资产总览 */
function baseInvoke(extra?: Record<string, unknown>) {
  mockInvoke.mockImplementation(
    invokeHandler(
      {
        list_currencies: mockCurrencies,
        list_accounts: [],
        list_categories: [],
        list_merchants: [],
        dashboard_overview: mockOverview,
      },
      extra,
    ),
  )
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
  localStorage.clear()
  // 每用例复位为 no-op，模拟「注册前」默认态，防模块级 sink 状态串扰
  resetToastSink()
  const store = useReferenceStore()
  await store.refresh()
})

describe('useDashboardOverview 首页净资产数据层（issue #143）', () => {
  it('加载 dashboard_overview 并装配出总览数据', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    const { overview, loading, error, refresh } = useDashboardOverview()
    await refresh()
    expect(loading.value).toBe(false)
    expect(error.value).toBeNull()
    expect(overview.value).toEqual(mockOverview)
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'dashboard_overview')).toBe(true)
    // 成功不弹 toast
    expect(sink.error).not.toHaveBeenCalled()
  })

  it('命令报错（如缺汇率）时进入兜底状态：overview 置空、error 带后端中文错误信息，不抛异常', async () => {
    baseInvoke({
      dashboard_overview: () => Promise.reject(new Error('缺少 USD→CNY 汇率，无法折算')),
    })
    const sink = makeFakeSink()
    registerToastSink(sink)
    const { overview, loading, error, refresh } = useDashboardOverview()
    await expect(refresh()).resolves.not.toThrow()
    expect(loading.value).toBe(false)
    expect(overview.value).toBeNull()
    expect(error.value).toBe('缺少 USD→CNY 汇率，无法折算')
    // 失败弹默认 toast（归一文案），error 状态与 toast 双通道共存
    expect(sink.error).toHaveBeenCalledTimes(1)
    expect(sink.error).toHaveBeenCalledWith('缺少 USD→CNY 汇率，无法折算')
  })

  it('非 Error 抛出值（如 Tauri 字符串错误）也能兜底为文案', async () => {
    baseInvoke({ dashboard_overview: () => Promise.reject('缺汇率') })
    const { error, refresh } = useDashboardOverview()
    await refresh()
    expect(error.value).toBe('缺汇率')
  })

  it('成功后再次报错：overview 清空并切换到错误态；再次成功则恢复', async () => {
    const { overview, error, refresh } = useDashboardOverview()
    await refresh()
    expect(overview.value).not.toBeNull()

    baseInvoke({
      dashboard_overview: () => Promise.reject(new Error('缺少 HKD→CNY 汇率')),
    })
    await refresh()
    expect(overview.value).toBeNull()
    expect(error.value).toBe('缺少 HKD→CNY 汇率')

    baseInvoke()
    await refresh()
    expect(overview.value).toEqual(mockOverview)
    expect(error.value).toBeNull()
  })

  it('失败 toast 只在失败那次弹出：错误态↔成功态往返中 sink 各就各位', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    baseInvoke({
      dashboard_overview: () => Promise.reject(new Error('缺少 HKD→CNY 汇率')),
    })
    const { refresh } = useDashboardOverview()
    await refresh()
    expect(sink.error).toHaveBeenCalledTimes(1)

    baseInvoke()
    await refresh()
    expect(sink.error).toHaveBeenCalledTimes(1)
  })
})
