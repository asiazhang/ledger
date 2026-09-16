import { describe, it, expect, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import {
  captureListenHandlers,
  mockListen,
  type CapturedListener,
} from '@ledger/test-support/listen-mock'
import HistoryBackfillIndicator from '@/investment/HistoryBackfillIndicator.vue'
import {
  HISTORY_BACKFILL_PROGRESS_EVENT,
  resetHistoryBackfillForTest,
} from '@/investment/useHistoryBackfill'

/**
 * 后台补全静默计数组件（issue #1375 / ADR-0122）：存在性断言——进度在途时
 * 渲染只读计数（删掉对应渲染即变红）；终态收起后不渲染。静默语义的可观察
 * 形态：无 role="progressbar"（不冒充手动同步的进度条形态）、无按钮、文案
 * 经 i18n。
 */

function fire(handlers: CapturedListener[], payload: unknown): void {
  for (const handler of handlers) handler({ event: HISTORY_BACKFILL_PROGRESS_EVENT, payload })
}

describe('HistoryBackfillIndicator 后台补全静默计数（投资页）', () => {
  let handlers: CapturedListener[]

  beforeEach(() => {
    mockListen.mockReset()
    resetHistoryBackfillForTest()
    handlers = captureListenHandlers()
  })

  it('无在途轮次不渲染', () => {
    const wrapper = mount(HistoryBackfillIndicator)
    expect(wrapper.find('[data-testid="history-backfill-indicator"]').exists()).toBe(false)
  })

  it('在途轮次渲染只读计数（存在性断言：删掉渲染即变红）', async () => {
    const wrapper = mount(HistoryBackfillIndicator)
    fire(handlers, { done: 137, total: 228 })
    await wrapper.vm.$nextTick()
    const indicator = wrapper.find('[data-testid="history-backfill-indicator"]')
    expect(indicator.exists()).toBe(true)
    expect(indicator.text()).toContain('历史数据补全中 137/228')
    // 静默语义：不是手动同步的进度条形态（无 progressbar role），无按钮。
    expect(indicator.find('[role="progressbar"]').exists()).toBe(false)
    expect(indicator.find('button').exists()).toBe(false)
  })

  it('基金首刷深回填期间另起一行渲染页级明细', async () => {
    const wrapper = mount(HistoryBackfillIndicator)
    fire(handlers, {
      done: 3,
      total: 228,
      fund: { code: '110022', page: 3, pages: 25 },
    })
    await wrapper.vm.$nextTick()
    expect(wrapper.find('[data-testid="history-backfill-indicator-fund"]').text()).toBe(
      '回填 110022：第 3/25 页',
    )
  })

  it('终态静默收起：done ≥ total 后组件消失', async () => {
    const wrapper = mount(HistoryBackfillIndicator)
    fire(handlers, { done: 1, total: 228 })
    await wrapper.vm.$nextTick()
    expect(wrapper.find('[data-testid="history-backfill-indicator"]').exists()).toBe(true)
    fire(handlers, { done: 228, total: 228 })
    await wrapper.vm.$nextTick()
    expect(wrapper.find('[data-testid="history-backfill-indicator"]').exists()).toBe(false)
  })
})
