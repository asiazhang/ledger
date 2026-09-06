import {
  setTxnDb,
  makeTxn,
  mountView,
  listCalls,
  lastListFilter,
  tablePagination,
  setReportDateRange,
  mockInvoke,
} from './common'
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NButton } from 'naive-ui'
import { captureListenHandlers, type CapturedListener } from '../helpers/listen-mock'
import type { Transaction } from '@/types'

/**
 * 交易页期间步进器与期间标签行为测试（issue #383 / #391）。
 *
 * 只测外部行为：步进后的过滤状态（起止日期快照）、列表刷新与翻页归零、
 * 禁用态（「全部」无游标；#391 起步进钳制于数据期间边界——边界末端箭头置灰，
 * 修订 #383「不钳制未来」；边界在途/失败退化为不钳制）、边界拉取时机
 * （挂载 + ledger:changed 失效重拉）与期间标签/芯片联动。
 * 区间 ⇄ 期间换算、步进跨界、边界派生与可达性判定见 time-period.test.ts 单测。
 * 今天是本文件的前提——以 fake timers 固定为 2026-01-15（本地），步进落点、
 * 高亮与「当前期间」随之确定。
 */
describe('TransactionsView 期间步进器（issue #383 / #391）', () => {
  // 富数据集：当月/上月（去年 12 月）/去年年中各一笔，月档边界随之 [2025-06, 2026-01]
  //（最新端被「今天抬升」托在当前期间）
  const stepDb: Transaction[] = [
    makeTxn(1, 'acc-1', { date: '2026-01-05' }),
    makeTxn(2, 'acc-1', { date: '2025-12-10' }),
    makeTxn(3, 'acc-1', { date: '2025-06-01' }),
  ]

  /** 捕获 ledger:changed 监听处理器（视图与各 store 在挂载/创建时注册）。 */
  let ledgerHandlers: CapturedListener[]

  beforeEach(() => {
    setTxnDb([...stepDb])
    // 固定「今天」：步进起点（当月 2026-01）与高亮随之确定
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 0, 15, 12, 0, 0))
    ledgerHandlers = captureListenHandlers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  /** 广播 ledger:changed（后端写库后的失效信号）：全部已注册处理器同形触发。 */
  async function fireLedgerChanged() {
    ledgerHandlers.forEach((h) => h({ event: 'ledger:changed', payload: null }))
    await flushPromises()
  }

  const rangeCalls = () => mockInvoke.mock.calls.filter(([cmd]) => cmd === 'report_date_range')

  /** 步进按钮按 aria-label 定位（图标按钮无文案）。 */
  const stepButton = (wrapper: ReturnType<typeof mount>, key: 'prev' | 'next') =>
    wrapper
      .findAllComponents(NButton)
      .find((b) => b.attributes('aria-label') === (key === 'prev' ? '上一个周期' : '下一个周期'))!

  const periodLabel = (wrapper: ReturnType<typeof mount>) =>
    wrapper.find('.period-label-text').text()

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
    expect(periodLabel(wrapper)).toBe('选择期间')
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

  it('界内空期间仍可达（2025-12 → 2025-11，空列表诚实显示）', async () => {
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

  it('钳制：> 在最新期间（2026-01）置灰，点击不触发请求', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    // 最新边界 = max(当前期间, 最新交易期间) = 2026-01 → 2026-02 不可达
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(true)
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(false)
    const before = listCalls().length
    await step(wrapper, 'next')
    expect(listCalls().length).toBe(before)
  })

  it('钳制：< 步进到最早期间（2025-06）后置灰，点击不触发请求', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    for (let i = 0; i < 7; i++) await step(wrapper, 'prev')
    expect(periodLabel(wrapper)).toBe('2025年6月')
    // 2025-05 早于最早交易期间 → 不可达；2025-07 仍在界内可走回
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(true)
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(false)
    const before = listCalls().length
    await step(wrapper, 'prev')
    expect(listCalls().length).toBe(before)
  })

  it('钳制随单位切换：当季（2026 一季度 = 最新）> 置灰、< 可步进', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当季')
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(true)
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(false)
  })

  it('钳制随单位切换：去年（年游标 2025）< 到 2024 置灰、> 可走回 2026', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '去年')
    expect(periodLabel(wrapper)).toBe('2025年')
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(true)
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(false)
  })

  it('退化：边界拉取失败时不钳制，> 可步进进无数据期间（空列表诚实显示）', async () => {
    const failing: Promise<{ min_date: string | null; max_date: string | null }> = Promise.reject(
      new Error('boom'),
    )
    failing.catch(() => {}) // 防 unhandled rejection 噪音：视图侧仍会收到拒绝
    setReportDateRange(failing)
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(false)
    await step(wrapper, 'next')
    expect(lastListFilter()).toMatchObject({ page: 1, from: '2026-02-01', to: '2026-02-28' })
    expect(wrapper.text()).toContain('共 0 条')
  })

  it('退化：边界在途时不钳制（不阻塞步进），到达后恢复钳制', async () => {
    let resolveRange: (r: { min_date: string; max_date: string }) => void = () => {}
    setReportDateRange(new Promise((resolve) => { resolveRange = resolve }))
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    // 在途：不钳制，可步进进未来期间
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(false)
    await step(wrapper, 'next')
    expect(lastListFilter()).toMatchObject({ from: '2026-02-01', to: '2026-02-28' })
    // 边界到达：游标 2026-02 晚于最新边界 2026-01 → > 置灰、< 可走回
    resolveRange({ min_date: '2025-06-01', max_date: '2026-01-05' })
    await flushPromises()
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(true)
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(false)
  })

  it('空库回退单当前期间：两向均不可步进', async () => {
    setTxnDb([])
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(true)
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(true)
  })

  it('边界拉取：挂载拉取一次 + ledger:changed 失效重拉即时外扩', async () => {
    // 单月数据：月档边界 [2026-01, 2026-01]，< 置灰
    setTxnDb([makeTxn(1, 'acc-1', { date: '2026-01-05' })])
    const wrapper = await mountView()
    expect(rangeCalls().length).toBe(1)
    await clickChip(wrapper, '当月')
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(true)
    // AI 导入外扩历史 → ledger:changed 重拉 → < 随新边界（2025-08）解锁
    setTxnDb([
      makeTxn(1, 'acc-1', { date: '2026-01-05' }),
      makeTxn(2, 'acc-1', { date: '2025-08-01' }),
    ])
    await fireLedgerChanged()
    expect(rangeCalls().length).toBe(2)
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(false)
  })

  it('边界拉取：ledger:changed 删除收窄即时跟随（箭头重新置灰）', async () => {
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    await step(wrapper, 'prev') // 游标 2025-12，界内
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(false)
    // 删光 2025 年流水 → 边界收窄为 [2026-01, 2026-01] → 2025-11 不可达
    setTxnDb([makeTxn(1, 'acc-1', { date: '2026-01-05' })])
    await fireLedgerChanged()
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(true)
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(false) // 2026-01 界内可走回
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
    // 混入一笔上月数据，使 <（→ 2025-12）仍在数据边界内可达
    setTxnDb([
      makeTxn(99, 'acc-1', { date: '2025-12-15' }),
      ...Array.from({ length: 25 }, (_, i) => makeTxn(i + 1, 'acc-1', { date: '2026-01-05' })),
    ])
    const wrapper = await mountView()
    await clickChip(wrapper, '当月')
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2 })
    await step(wrapper, 'prev')
    expect(lastListFilter()).toMatchObject({ page: 1, from: '2025-12-01', to: '2025-12-31' })
  })
})
