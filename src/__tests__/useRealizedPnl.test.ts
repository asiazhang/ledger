import { describe, it, expect, vi, beforeEach } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { defineComponent } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import { useRealizedPnl } from '@/composables/useRealizedPnl'
import { registerToastSink } from '@/composables/useLoadable'
import type { RealizedPnlSummary } from '@/types'
import {
  invokeHandler,
  makeFakeSink,
  makeInstrument,
  makePnlSummary,
  mockAccounts,
  mockCurrencies,
  resetToastSink,
} from './factories'

const mockInvoke = vi.mocked(invoke)

const mockSummary = makePnlSummary()

/** 默认 invoke mock：参考数据 + 已实现盈亏汇总 + 标的搜索 */
function baseInvoke(extra?: Record<string, unknown>) {
  mockInvoke.mockImplementation(
    invokeHandler(
      {
        list_currencies: mockCurrencies,
        list_accounts: mockAccounts,
        list_categories: [],
        list_merchants: [],
        realized_pnl_summary: mockSummary,
        list_instruments: { items: [makeInstrument({ id: 'inst-1' })], total: 1 },
      },
      extra,
    ),
  )
}

/** 宿主组件：模拟盈亏页在 setup 内使用 composable（onMounted 自动首刷时序留在薄壳内） */
const Host = defineComponent({
  setup() {
    return { shell: useRealizedPnl() }
  },
  template: '<div />',
})

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
  // 每用例复位为 no-op，模拟「注册前」默认态，防模块级 sink 状态串扰
  resetToastSink()
  const store = useReferenceStore()
  await store.refresh()
})

describe('useRealizedPnl 已实现盈亏数据层', () => {
  it('加载已实现盈亏汇总并派生 totalPnl', async () => {
    const { summary, loading, totalPnl, refresh } = useRealizedPnl()
    expect(totalPnl.value).toBe(0) // 未加载前空态
    await refresh()
    expect(loading.value).toBe(false)
    expect(summary.value).toEqual(mockSummary)
    expect(totalPnl.value).toBe(30000)
  })

  it('无筛选时不带 filter 参数（后端全表口径）', async () => {
    const { refresh } = useRealizedPnl()
    await refresh()
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'realized_pnl_summary')
    expect(call![1]).toEqual({ filter: null })
  })

  it('竞态：后发覆盖先发，迟到前发结果不覆写 summary 终态', async () => {
    let releaseFirst!: (summary: RealizedPnlSummary) => void
    let calls = 0
    baseInvoke({
      realized_pnl_summary: () => {
        calls += 1
        if (calls === 1) {
          // 先发慢请求：手动放行，制造「后发已终态、先发迟到」的交错
          return new Promise<RealizedPnlSummary>((resolve) => {
            releaseFirst = resolve
          })
        }
        return Promise.resolve(makePnlSummary({ total_realized_pnl_cents: 777 }))
      },
    })
    const { summary, error, refresh } = useRealizedPnl()
    const first = refresh()
    const second = refresh()
    await second
    expect(summary.value!.total_realized_pnl_cents).toBe(777)

    // 迟到的先发结果：已被 Loadable 竞态裁决作废为空，不覆写终态、不置 error
    releaseFirst(mockSummary)
    await first
    expect(summary.value!.total_realized_pnl_cents).toBe(777)
    expect(error.value).toBeNull()
  })

  it('账户/标的筛选生效：refresh 闭包自读当前筛选（0 元闭包，发起时点即最新值）', async () => {
    const { selectedAccountId, selectedInstrumentId, refresh } = useRealizedPnl()
    await refresh()

    selectedAccountId.value = 'acc-1'
    await refresh()
    const accountCall = mockInvoke.mock.calls.filter(
      ([cmd]) => cmd === 'realized_pnl_summary',
    )[1]
    expect(accountCall![1]).toEqual({ filter: { account_id: 'acc-1' } })

    selectedInstrumentId.value = 'inst-1'
    await refresh()
    const bothCall = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'realized_pnl_summary')[2]
    expect(bothCall![1]).toEqual({ filter: { account_id: 'acc-1', instrument_id: 'inst-1' } })
  })

  it('onSelectInstrument 更新标的筛选并立即刷新', async () => {
    const { selectedInstrumentId, onSelectInstrument } = useRealizedPnl()
    onSelectInstrument('inst-1')
    await flushPromises()
    expect(selectedInstrumentId.value).toBe('inst-1')
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'realized_pnl_summary')
    expect(calls.length).toBe(1)
    expect(calls[0]![1]).toEqual({ filter: { instrument_id: 'inst-1' } })
  })

  it('账户选项读参考数据单一来源', () => {
    const { accountOptions } = useRealizedPnl()
    expect(accountOptions.value).toEqual([{ label: '证券账户A', value: 'acc-1' }])
  })
})

describe('useRealizedPnl 标的远程搜索（防抖 + 刻意吞错，不收编）', () => {
  it('防抖后携带 search 参数远程搜索，仅最后一次触发生效', async () => {
    vi.useFakeTimers()
    const { searchInstruments, pnlInstrumentOptions } = useRealizedPnl()
    searchInstruments('浦发')
    searchInstruments('浦发银')
    await vi.advanceTimersByTimeAsync(300)
    await flushPromises()
    vi.useRealTimers()
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments')
    expect(calls.length).toBe(1)
    expect(calls[0]![1]).toEqual({ filter: { search: '浦发银', page_size: 50 } })
    expect(pnlInstrumentOptions.value).toEqual([{ label: '600000 · 浦发银行', value: 'inst-1' }])
  })

  it('搜索失败刻意吞错：静默清空选项，不置 error、不弹 toast（词汇表「刻意静默不收编」合法形态）', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    baseInvoke({ list_instruments: () => Promise.reject(new Error('搜索失败')) })
    vi.useFakeTimers()
    const { searchInstruments, error, searchingInstruments } = useRealizedPnl()
    searchInstruments('浦发')
    await vi.advanceTimersByTimeAsync(300)
    await flushPromises()
    vi.useRealTimers()
    expect(error.value).toBeNull()
    expect(searchingInstruments.value).toBe(false)
    expect(sink.error).not.toHaveBeenCalled()
  })
})

describe('useRealizedPnl 失败治愈（issue #325 Loadable 薄壳化）', () => {
  it('刷新失败不向调用方抛出：error 置位、loading 收尾、summary 保持原值不清空', async () => {
    const { summary, loading, error, refresh } = useRealizedPnl()
    await refresh()
    expect(summary.value).toEqual(mockSummary)

    baseInvoke({ realized_pnl_summary: () => Promise.reject(new Error('数据库文件已锁定')) })
    await expect(refresh()).resolves.not.toThrow()
    expect(loading.value).toBe(false)
    expect(error.value).toBe('数据库文件已锁定')
    expect(summary.value).toEqual(mockSummary)
  })

  it('失败弹默认 toast（serde 对象错误归一取 message），成功不弹——error 状态与 toast 双通道共存', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    baseInvoke({
      realized_pnl_summary: () => Promise.reject({ kind: 'db', message: '盈亏汇总查询失败' }),
    })
    const { error, refresh } = useRealizedPnl()
    await refresh()
    expect(error.value).toBe('盈亏汇总查询失败')
    expect(sink.error).toHaveBeenCalledTimes(1)
    expect(sink.error).toHaveBeenCalledWith('盈亏汇总查询失败')

    baseInvoke()
    await refresh()
    expect(sink.error).toHaveBeenCalledTimes(1)
  })

  it('失败后重试成功：error 清零、summary 重新填充（error 是唯一成败判据）', async () => {
    baseInvoke({ realized_pnl_summary: () => Promise.reject('首刷失败') })
    const { summary, error, refresh } = useRealizedPnl()
    await refresh()
    expect(error.value).toBe('首刷失败')
    expect(summary.value).toBeNull()

    baseInvoke()
    await refresh()
    expect(error.value).toBeNull()
    expect(summary.value).toEqual(mockSummary)
  })

  it('挂载首刷失败（onMounted 自动首刷）：不再产生未处理 rejection，进入 error 终态并弹 toast', async () => {
    baseInvoke({ realized_pnl_summary: () => Promise.reject(new Error('首刷失败')) })
    const sink = makeFakeSink()
    registerToastSink(sink)
    const wrapper = mount(Host)
    await flushPromises()
    expect(wrapper.vm.shell.error.value).toBe('首刷失败')
    expect(wrapper.vm.shell.summary.value).toBeNull()
    expect(sink.error).toHaveBeenCalledWith('首刷失败')
  })
})
