import { describe, it, expect, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { deferred } from '@ledger/test-support/deferred'
import { mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { withSetup } from '@ledger/test-support/mount'
import { registerToastSink } from '@ledger/loadable'
import { SEARCH_DEBOUNCE_MS } from '@/composables/search-debounce'
import { useInstrumentSearch } from '@/investment/useInstrumentSearch'
import { makeFakeSink, makeInstrument, resetToastSink } from './factories'
import type { Instrument } from '@ledger/types'

/**
 * 标的远程搜索编排的机制断言单点（issue #1308 决策 7）：防抖只最后一次生效、
 * 空查询清空且不发请求、请求形状、失败静默、在途竞态作废（#1401）全部钉在本
 * 文件；两处消费方（useInvestmentForm / useRealizedPnl）只保留各自领域断言
 * （候选投影/合并、基金判定、select 回填），机制断言不再两处复制。
 */

/** list_instruments 应答夹具：symbol 即查询词，便于断言是哪次查询的结果落位 */
function resultFor(symbol: string, name: string): { items: Instrument[]; total: number } {
  return { items: [makeInstrument({ id: `ins-${symbol}`, symbol, name })], total: 1 }
}

function listInstrumentCalls(): unknown[][] {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments')
}

beforeEach(() => {
  // 每用例复位为 no-op，模拟「注册前」默认态，防模块级 sink 状态串扰
  resetToastSink()
})

describe('useInstrumentSearch 标的远程搜索编排（issue #1308）', () => {
  it('防抖：连续输入只发最后一次查询，候选随结果落位', async () => {
    vi.useFakeTimers()
    try {
      wireInvokeSeam({ defaults: { list_instruments: resultFor('浦发银行', '后查') } })
      const { search, items, searching } = withSetup(() => useInstrumentSearch())
      search('浦发')
      search('浦发银')
      search('浦发银行')
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
      await flushPromises()

      const calls = listInstrumentCalls()
      expect(calls.length).toBe(1)
      // 双断言：调用事实之外，候选确随最后一次查询落位
      expect(items.value.map((i) => i.symbol)).toEqual(['浦发银行'])
      expect(searching.value).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })

  it('请求形状：{ search, page_size: 50 }，查询词 trim 后携带', async () => {
    vi.useFakeTimers()
    try {
      wireInvokeSeam({ defaults: { list_instruments: resultFor('NVDA', '英伟达') } })
      const { search, items } = withSetup(() => useInstrumentSearch())
      search('  NVDA  ')
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
      await flushPromises()

      const calls = listInstrumentCalls()
      expect(calls.length).toBe(1)
      expect(calls[0]![1]).toEqual({ filter: { search: 'NVDA', page_size: 50 } })
      expect(items.value.map((i) => i.symbol)).toEqual(['NVDA'])
    } finally {
      vi.useRealTimers()
    }
  })

  it('空查询（trim 后为空）不发请求：候选清空、searching 置收', async () => {
    vi.useFakeTimers()
    try {
      wireInvokeSeam({ defaults: { list_instruments: resultFor('NVDA', '英伟达') } })
      const { search, items, searching } = withSetup(() => useInstrumentSearch())
      // 前置：先经一次非空查询让候选非空（否则「清空」断言假绿）
      search('NVDA')
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
      await flushPromises()
      expect(items.value.length).toBe(1)

      const callsBefore = listInstrumentCalls().length
      search('   ')
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
      await flushPromises()

      expect(listInstrumentCalls().length).toBe(callsBefore)
      expect(items.value).toEqual([])
      expect(searching.value).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })

  it('失败刻意吞错：静默清空候选、searching 置收，无 toast 不置 error 通道', async () => {
    vi.useFakeTimers()
    try {
      const sink = makeFakeSink()
      registerToastSink(sink)
      // 一次性失败属动态行为：走 overrides 表（defaults 只收静态快照）
      wireInvokeSeam({
        overrides: { list_instruments: () => Promise.reject(new Error('搜索失败')) },
      })
      const { search, items, searching } = withSetup(() => useInstrumentSearch())
      search('浦发')
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
      await flushPromises()

      expect(items.value).toEqual([])
      expect(searching.value).toBe(false)
      expect(sink.error).not.toHaveBeenCalled()
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('useInstrumentSearch 在途竞态（issue #1401 编排内化）', () => {
  it('先发请求迟到不覆盖后发结果：候选呈现后发查询的标的', async () => {
    vi.useFakeTimers()
    try {
      const first = deferred<{ items: Instrument[]; total: number }>()
      const second = deferred<{ items: Instrument[]; total: number }>()
      let calls = 0
      wireInvokeSeam({
        overrides: {
          list_instruments: () => {
            calls += 1
            return calls === 1 ? first.promise : second.promise
          },
        },
      })
      const { search, items, searching } = withSetup(() => useInstrumentSearch())
      search('AAA')
      // 各自推进过防抖窗口：两次查询都实际发出，才构成乱序到达的竞态；
      // 请求挂起未决期间 searching 置位（在途标志升起，终态置收才非空断言）
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
      expect(searching.value).toBe(true)
      search('BBB')
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
      expect(calls).toBe(2)

      // 后发先到，先发迟到：呈现的应是后发查询的结果（删掉纪元守卫本断言变红）
      second.resolve(resultFor('BBB', '后发'))
      await flushPromises()
      first.resolve(resultFor('AAA', '先发'))
      await flushPromises()

      expect(items.value.map((i) => i.symbol)).toEqual(['BBB'])
      expect(searching.value).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })

  it('清空输入作废在途：先前发出的非空请求迟到不再填回候选', async () => {
    vi.useFakeTimers()
    try {
      const pending = deferred<{ items: Instrument[]; total: number }>()
      let calls = 0
      wireInvokeSeam({
        overrides: {
          list_instruments: () => {
            calls += 1
            return pending.promise
          },
        },
      })
      const { search, items, searching } = withSetup(() => useInstrumentSearch())
      search('AAA')
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
      // 请求确实已发出（否则本用例会退化为「从未在途」而假绿）
      expect(calls).toBe(1)
      search('')
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS) // 空查询落地清空
      expect(items.value).toEqual([])

      pending.resolve(resultFor('AAA', '先发'))
      await flushPromises()
      expect(items.value).toEqual([])
      expect(searching.value).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })
})
