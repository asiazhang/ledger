import { setTxnDb, makeTxn, mountView, listCalls, lastListFilter, tablePagination } from './common'
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NButton, NDatePicker } from 'naive-ui'
import type { Transaction } from '@/types'

/**
 * 交易页时间维度行行为测试（issue #382）。
 *
 * 只测外部行为：点芯片后的过滤状态（起止日期快照）、列表刷新与翻页归零、
 * 芯片点亮可见态、清除筛选与 URL 复位回「全部」。芯片换算逻辑（预设 ⇄ 区间、
 * 高亮匹配）的边界单测见 time-period.test.ts。今天是本文件的前提——
 * 以 fake timers 固定为 2026-01-15（本地），芯片换算结果随之确定。
 */
describe('TransactionsView 时间维度行（issue #382）', () => {
  // 富数据集：当月/当月外/去年各一笔，供刷新结果区分断言
  const chipDb: Transaction[] = [
    makeTxn(1, 'acc-1', { date: '2026-01-05' }),
    makeTxn(2, 'acc-1', { date: '2026-02-10' }),
    makeTxn(3, 'acc-1', { date: '2025-06-01' }),
  ]

  beforeEach(() => {
    setTxnDb([...chipDb])
    // 固定「今天」：芯片快照与高亮随之确定（2026-01-15 → 当月 2026-01、当季 2026Q1）
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 0, 15, 12, 0, 0))
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  /** 芯片按钮按文案定位（文案唯一：全部/当月/当季/当年/去年）。 */
  const chip = (wrapper: ReturnType<typeof mount>, label: string) =>
    wrapper.findAllComponents(NButton).find((b) => b.text().trim() === label)!

  async function clickChip(wrapper: ReturnType<typeof mount>, label: string) {
    await chip(wrapper, label).trigger('click')
    await flushPromises()
  }

  const lit = (wrapper: ReturnType<typeof mount>, label: string) =>
    chip(wrapper, label).props('type') === 'primary'

  it('默认态：「全部」点亮、其余熄灭，首刷请求不带日期参数', async () => {
    const wrapper = await mountView()
    expect(lit(wrapper, '全部')).toBe(true)
    for (const label of ['当月', '当季', '当年', '去年']) {
      expect(lit(wrapper, label)).toBe(false)
    }
    expect(lastListFilter()).not.toHaveProperty('from')
    expect(lastListFilter()).not.toHaveProperty('to')
    expect(wrapper.text()).toContain('共 3 条')
  })

  it('两个日期选择控件不再出现在交易页（搜索页保留任意区间）', async () => {
    const wrapper = await mountView()
    expect(wrapper.findAllComponents(NDatePicker).length).toBe(0)
  })

  it('点「当月」写入当月快照：from/to 传后端、列表刷新、「当月」点亮「全部」熄灭', async () => {
    const wrapper = await mountView()
    const before = listCalls().length
    await clickChip(wrapper, '当月')
    expect(listCalls().length).toBe(before + 1)
    expect(lastListFilter()).toMatchObject({ page: 1, from: '2026-01-01', to: '2026-01-31' })
    expect(wrapper.text()).toContain('共 1 条')
    expect(lit(wrapper, '当月')).toBe(true)
    expect(lit(wrapper, '全部')).toBe(false)
  })

  it('五枚日期芯片各自写入对应自然周期快照（当季/当年/去年）', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当季')
    expect(lastListFilter()).toMatchObject({ from: '2026-01-01', to: '2026-03-31' })
    expect(wrapper.text()).toContain('共 2 条')
    await clickChip(wrapper, '当年')
    expect(lastListFilter()).toMatchObject({ from: '2026-01-01', to: '2026-12-31' })
    expect(wrapper.text()).toContain('共 2 条')
    await clickChip(wrapper, '去年')
    expect(lastListFilter()).toMatchObject({ from: '2025-01-01', to: '2025-12-31' })
    // 去年区间恰有 1 笔（2025-06-01），列表诚实刷新
    expect(wrapper.text()).toContain('共 1 条')
    expect(lit(wrapper, '去年')).toBe(true)
  })

  it('重复点同一芯片不重复刷新（同值守卫：快照未变不动作）', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    const before = listCalls().length
    await clickChip(wrapper, '当月')
    expect(listCalls().length).toBe(before)
  })

  it('切换芯片单选：后点覆盖前点快照，仅后点芯片点亮', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    expect(lit(wrapper, '当月')).toBe(true)
    await clickChip(wrapper, '去年')
    expect(lit(wrapper, '去年')).toBe(true)
    expect(lit(wrapper, '当月')).toBe(false)
    expect(lastListFilter()).toMatchObject({ from: '2025-01-01', to: '2025-12-31' })
  })

  it('清除筛选把日期维度一并还原为「全部」并回全量列表', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    expect(wrapper.text()).toContain('共 1 条')
    const clear = wrapper
      .findAllComponents(NButton)
      .find((b) => b.text().includes('清除筛选'))!
    await clear.trigger('click')
    await flushPromises()
    const f = lastListFilter()
    expect(f).not.toHaveProperty('from')
    expect(f).not.toHaveProperty('to')
    expect(f).toMatchObject({ page: 1 })
    expect(wrapper.text()).toContain('共 3 条')
    expect(lit(wrapper, '全部')).toBe(true)
    expect(lit(wrapper, '当月')).toBe(false)
  })

  it('URL 下钻后导航清除参数：既有复位行为使日期维度回「全部」', async () => {
    const { routeMock } = await import('./common')
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    expect(lastListFilter()).toMatchObject({ from: '2026-01-01', to: '2026-01-31' })
    // 导航清除 query → 模块复位日期/类型（#96 决策 3）→「全部」重新点亮
    routeMock.query = {}
    await flushPromises()
    const f = lastListFilter()
    expect(f).not.toHaveProperty('from')
    expect(f).not.toHaveProperty('to')
    expect(lit(wrapper, '全部')).toBe(true)
    expect(lit(wrapper, '当月')).toBe(false)
  })

  it('切换时间维度后翻页归零：从第 2 页点其他芯片回到第 1 页', async () => {
    // 25 条当月数据 → 第 1 页 20 条 + 第 2 页 5 条
    setTxnDb(Array.from({ length: 25 }, (_, i) => makeTxn(i + 1, 'acc-1', { date: '2026-01-05' })))
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    expect(wrapper.text()).toContain('共 25 条')
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2 })
    await clickChip(wrapper, '当季')
    expect(lastListFilter()).toMatchObject({ page: 1, from: '2026-01-01', to: '2026-03-31' })
  })
})
