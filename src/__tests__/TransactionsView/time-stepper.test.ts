import { setTxnDb, makeTxn, mountView, listCalls, lastListFilter, tablePagination } from './common'
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NButton } from 'naive-ui'
import type { Transaction } from '@/types'

/**
 * 交易页期间步进器与期间标签行为测试（issue #383）。
 *
 * 只测外部行为：步进后的过滤状态（起止日期快照）、列表刷新与翻页归零、
 * 禁用态（「全部」无游标）、期间标签更新与芯片点亮联动。区间 ⇄ 期间换算、
 * 步进跨界与标签格式的边界单测见 time-period.test.ts。今天是本文件的前提——
 * 以 fake timers 固定为 2026-01-15（本地），步进落点与高亮随之确定。
 */
describe('TransactionsView 期间步进器（issue #383）', () => {
  // 富数据集：当月/上月（去年 12 月）/去年年中各一笔，供步进结果区分断言
  const stepDb: Transaction[] = [
    makeTxn(1, 'acc-1', { date: '2026-01-05' }),
    makeTxn(2, 'acc-1', { date: '2025-12-10' }),
    makeTxn(3, 'acc-1', { date: '2025-06-01' }),
  ]

  beforeEach(() => {
    setTxnDb([...stepDb])
    // 固定「今天」：步进起点（当月 2026-01）与高亮随之确定
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 0, 15, 12, 0, 0))
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  /** 步进按钮按 aria-label 定位（图标按钮无文案）。 */
  const stepButton = (wrapper: ReturnType<typeof mount>, key: 'prev' | 'next') =>
    wrapper
      .findAllComponents(NButton)
      .find((b) => b.attributes('aria-label') === (key === 'prev' ? '上一个周期' : '下一个周期'))!

  const periodLabel = (wrapper: ReturnType<typeof mount>) =>
    wrapper.find('.period-label').text()

  async function clickChip(wrapper: ReturnType<typeof mount>, label: string) {
    const chip = wrapper
      .findAllComponents(NButton)
      .find((b) => b.text().trim() === label)!
    await chip.trigger('click')
    await flushPromises()
  }

  async function step(wrapper: ReturnType<typeof mount>, key: 'prev' | 'next') {
    await stepButton(wrapper, key).trigger('click')
    await flushPromises()
  }

  const lit = (wrapper: ReturnType<typeof mount>, label: string) => {
    const chip = wrapper
      .findAllComponents(NButton)
      .find((b) => b.text().trim() === label)!
    return chip.props('type') === 'primary'
  }

  it('「全部」默认态：步进置灰、标签为占位符，点击不触发请求', async () => {
    const wrapper = await mountView()
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(true)
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(true)
    expect(periodLabel(wrapper)).toBe('—')
    const before = listCalls().length
    await step(wrapper, 'prev')
    await step(wrapper, 'next')
    expect(listCalls().length).toBe(before)
  })

  it('点「当月」后 < 步进上月（跨年回退 2026-01 → 2025-12）：区间写回、标签更新、芯片全灭', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    expect(periodLabel(wrapper)).toBe('2026年1月')
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(false)
    const before = listCalls().length
    await step(wrapper, 'prev')
    expect(listCalls().length).toBe(before + 1)
    expect(lastListFilter()).toMatchObject({ page: 1, from: '2025-12-01', to: '2025-12-31' })
    expect(periodLabel(wrapper)).toBe('2025年12月')
    // 历史月份不是任何预设定义 → 芯片全灭，列表快照不漂移
    for (const label of ['全部', '当月', '当季', '当年', '去年']) {
      expect(lit(wrapper, label)).toBe(false)
    }
  })

  it('连续 < 可达任意历史周期（2025-12 → 2025-11，空列表诚实显示）', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    await step(wrapper, 'prev')
    await step(wrapper, 'prev')
    expect(lastListFilter()).toMatchObject({ from: '2025-11-01', to: '2025-11-30' })
    expect(wrapper.text()).toContain('共 0 条')
  })

  it('> 原路走回：2025-12 → 2026-01 后「当月」芯片重新点亮', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    await step(wrapper, 'prev')
    await step(wrapper, 'next')
    expect(lastListFilter()).toMatchObject({ from: '2026-01-01', to: '2026-01-31' })
    expect(periodLabel(wrapper)).toBe('2026年1月')
    expect(lit(wrapper, '当月')).toBe(true)
  })

  it('> 不钳制未来期间：当月 > 一步到 2026-02，空列表是诚实行为', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    await step(wrapper, 'next')
    expect(lastListFilter()).toMatchObject({ page: 1, from: '2026-02-01', to: '2026-02-28' })
    expect(wrapper.text()).toContain('共 0 条')
  })

  it('季期间逐季步进（含跨年）：当季 < → 2025年四季度', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当季')
    expect(periodLabel(wrapper)).toBe('2026年一季度')
    await step(wrapper, 'prev')
    expect(lastListFilter()).toMatchObject({ from: '2025-10-01', to: '2025-12-31' })
    expect(periodLabel(wrapper)).toBe('2025年四季度')
    expect(wrapper.text()).toContain('共 1 条')
  })

  it('年期间逐年步进：当年 < → 2025 年，落点恰为「去年」预设 → 芯片点亮', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当年')
    expect(periodLabel(wrapper)).toBe('2026年')
    await step(wrapper, 'prev')
    expect(lastListFilter()).toMatchObject({ from: '2025-01-01', to: '2025-12-31' })
    expect(periodLabel(wrapper)).toBe('2025年')
    expect(lit(wrapper, '去年')).toBe(true)
    expect(lit(wrapper, '当年')).toBe(false)
  })

  it('步进后翻页归零：第 2 页 < 一步回第 1 页', async () => {
    setTxnDb(Array.from({ length: 25 }, (_, i) => makeTxn(i + 1, 'acc-1', { date: '2026-01-05' })))
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2 })
    await step(wrapper, 'prev')
    expect(lastListFilter()).toMatchObject({ page: 1, from: '2025-12-01', to: '2025-12-31' })
  })
})
