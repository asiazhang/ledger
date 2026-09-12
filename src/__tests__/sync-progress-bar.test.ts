import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import SyncProgressBar from '@/components/investments/SyncProgressBar.vue'
import type { InstrumentSyncProgress } from '@ledger/types'

/**
 * 同步进度条展示组件（issue #897 / ADR-0095）：只测外部行为——形态为
 * GlobalBusyBar 同款细条的确定进度版，蓝色区分；条旁一行计数文案经 i18n
 * 随界面语言；无进度（null）不渲染。两入口（盈亏页/标的页）渲染同一份组件，
 * 行为零分叉由 props 契约保证。
 */

function mountBar(progress: InstrumentSyncProgress | null) {
  return mount(SyncProgressBar, { props: { progress } })
}

describe('SyncProgressBar 同步进度条（issue #897）', () => {
  it('无进度（null）不渲染', () => {
    const wrapper = mountBar(null)
    expect(wrapper.find('[data-testid="instrument-sync-progress"]').exists()).toBe(false)
  })

  it('渲染计数文案与进度宽度（done/total → 百分比）', () => {
    const wrapper = mountBar({ done: 37, total: 100 })
    const bar = wrapper.find('[data-testid="instrument-sync-progress"]')
    expect(bar.exists()).toBe(true)
    // 中文文案形如「同步标的信息 37/100」（i18n 单点）
    expect(bar.text()).toContain('同步标的信息 37/100')
    expect(bar.find('[data-testid="instrument-sync-progress-bar"]').attributes('style')).toContain('width: 37%')
  })

  it('进度语义可访问：role=progressbar 与 aria 值域/当前值', () => {
    const wrapper = mountBar({ done: 37, total: 100 })
    const bar = wrapper.find('[role="progressbar"]')
    expect(bar.exists()).toBe(true)
    expect(bar.attributes('aria-valuemin')).toBe('0')
    expect(bar.attributes('aria-valuemax')).toBe('100')
    expect(bar.attributes('aria-valuenow')).toBe('37')
    expect(bar.attributes('aria-label')).toBe('同步标的信息进度')
  })

  it('done 为 0 时渲染 0% 起点而非隐藏', () => {
    const wrapper = mountBar({ done: 0, total: 100 })
    expect(wrapper.text()).toContain('同步标的信息 0/100')
    expect(wrapper.find('[data-testid="instrument-sync-progress-bar"]').attributes('style')).toContain('width: 0%')
  })

  it('基金深回填期间另起一行渲染页级明细，不改标的级进度宽度（issue #1061）', () => {
    // 单只基金首刷要翻约 25 页、按全局限速接近一分钟——页级明细让这一段
    // 「看起来卡死」的时间可见；进度条宽度仍按标的级 done/total，页推进不虚报。
    const wrapper = mountBar({
      done: 5,
      total: 100,
      fund: { code: '110022', page: 3, pages: 25 },
    })
    const bar = wrapper.find('[data-testid="instrument-sync-progress"]')
    expect(bar.text()).toContain('同步标的信息 5/100')
    expect(wrapper.find('[data-testid="instrument-sync-progress-fund"]').text()).toBe(
      '回填 110022：第 3/25 页',
    )
    expect(wrapper.find('[data-testid="instrument-sync-progress-bar"]').attributes('style')).toContain('width: 5%')
  })

  it('标的级推进（无页级明细）不渲染明细行', () => {
    const wrapper = mountBar({ done: 5, total: 100 })
    expect(wrapper.find('[data-testid="instrument-sync-progress-fund"]').exists()).toBe(false)
  })
})
