import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { NButton, NDatePicker } from 'naive-ui'
import { resetOverlays, hasOpenOverlay, openOverlayNames } from '@/composables/overlayRegistry'
import AppDatePicker from '@/components/AppDatePicker.vue'
import QuickTimeRange from '@/components/QuickTimeRange.vue'
import { DATED_TIME_PERIOD_PRESETS, type NullableDateRange } from '@/utils/time-period'

const mockInvoke = vi.mocked(invoke)

// jsdom 未实现元素滚动（naive-ui 日期面板打开时会 scrollTo），补空实现避免
// 打断 Vue 调度队列（仅影响本文件的弹层交互用例）。
beforeAll(() => {
  Element.prototype.scrollTo = () => {}
})

/**
 * 时间范围快捷选择共享受控组件行为测试（issue #410，#409 接缝 2 唯一新缝）。
 *
 * 只测外部行为：点芯片 emit 快照区间、步进换算、面板选择、边界外步进置灰、
 * 面板开/关上报弹层注册表（Overlay Suppression）。组件受控不持状态源——
 * 高亮与步进游标全部由 prop 区间派生，组件产出只经 update:modelValue 回流调用方
 * （测试以 setProps 模拟调用方消费 v-model）。换算与边界数学单测见 time-period.test.ts；
 * 交易页消费后的视图级行为由 TransactionsView 的 time-chips / time-stepper 测试锚定
 * （断言不改）。今天是本文件的前提——固定「今天」假计时器为 2026-01-15（本地），
 * 预设定义、高亮与当前期间随之确定。
 */
describe('QuickTimeRange 共享受控组件（issue #410）', () => {
  /** 数据期间边界原始日期对：月档边界 [2025-06, 2026-01]（最新端被「今天」抬升托在当前期间）。 */
  const BOUNDARY = { min_date: '2025-06-01', max_date: '2026-01-05' }

  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 0, 15, 12, 0, 0))
    resetOverlays()
    mockInvoke.mockReset()
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'report_date_range') return Promise.resolve(BOUNDARY)
      if (cmd === 'list_insurers') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
  })

  afterEach(() => {
    vi.useRealTimers()
    resetOverlays()
  })

  function mountRange(modelValue: NullableDateRange = { from: null, to: null }) {
    return mount(QuickTimeRange, { props: { modelValue } })
  }

  const chip = (wrapper: VueWrapper, label: string) =>
    wrapper.findAllComponents(NButton).find((b) => b.text().trim() === label)!

  async function clickChip(wrapper: VueWrapper, label: string) {
    await chip(wrapper, label).trigger('click')
    await flushPromises()
  }

  const lit = (wrapper: VueWrapper, label: string) =>
    chip(wrapper, label).props('type') === 'primary'

  const lastEmitted = (wrapper: VueWrapper): NullableDateRange | undefined => {
    const events = wrapper.emitted('update:modelValue') as Array<[NullableDateRange]> | undefined
    return events?.[events.length - 1]?.[0]
  }

  const emitCount = (wrapper: VueWrapper) => wrapper.emitted('update:modelValue')?.length ?? 0

  const stepButton = (wrapper: VueWrapper, key: 'prev' | 'next') =>
    wrapper
      .findAllComponents(NButton)
      .find((b) => b.attributes('aria-label') === (key === 'prev' ? '上一个周期' : '下一个周期'))!

  const periodLabel = (wrapper: VueWrapper) => wrapper.find('.period-label-text').text()

  /** 步进一步并模拟调用方消费 v-model（受控契约：prop 前进后游标随之前进）。 */
  async function stepAndConsume(wrapper: VueWrapper, key: 'prev' | 'next') {
    await stepButton(wrapper, key).trigger('click')
    await flushPromises()
    const next = lastEmitted(wrapper)!
    await wrapper.setProps({ modelValue: next })
    return next
  }

  it('默认态（双空区间）：五枚芯片、「全部」点亮、步进双向置灰、标签占位、挂载拉取边界一次', async () => {
    const wrapper = mountRange()
    await flushPromises()
    for (const label of ['全部', '当月', '当季', '当年', '去年']) {
      expect(chip(wrapper, label).exists()).toBe(true)
    }
    expect(lit(wrapper, '全部')).toBe(true)
    for (const label of ['当月', '当季', '当年', '去年']) {
      expect(lit(wrapper, label)).toBe(false)
    }
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(true)
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(true)
    expect(periodLabel(wrapper)).toBe('选择期间')
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === 'report_date_range')).toHaveLength(1)
  })

  it('点日期芯片 emit 精确自然周期快照（当月/当季/当年/去年各按定义换算）', async () => {
    const wrapper = mountRange()
    await flushPromises()
    await clickChip(wrapper, '当月')
    expect(lastEmitted(wrapper)).toEqual({ from: '2026-01-01', to: '2026-01-31' })
    await clickChip(wrapper, '当季')
    expect(lastEmitted(wrapper)).toEqual({ from: '2026-01-01', to: '2026-03-31' })
    await clickChip(wrapper, '当年')
    expect(lastEmitted(wrapper)).toEqual({ from: '2026-01-01', to: '2026-12-31' })
    await clickChip(wrapper, '去年')
    expect(lastEmitted(wrapper)).toEqual({ from: '2025-01-01', to: '2025-12-31' })
  })

  it('点「全部」emit 双空区间（无日期过滤 = 默认态）', async () => {
    const wrapper = mountRange({ from: '2026-01-01', to: '2026-01-31' })
    await flushPromises()
    await clickChip(wrapper, '全部')
    expect(lastEmitted(wrapper)).toEqual({ from: null, to: null })
  })

  it('受控高亮：点亮纯由 prop 区间派生，组件不自持选择状态', async () => {
    // 「去年」快照 → 恰为预设定义，点亮「去年」
    const wrapper = mountRange({ from: '2025-01-01', to: '2025-12-31' })
    await flushPromises()
    expect(lit(wrapper, '去年')).toBe(true)
    expect(lit(wrapper, '全部')).toBe(false)
    // 历史月份（非预设定义）→ 无芯片点亮，列表快照不漂移
    const historical = mountRange({ from: '2025-12-01', to: '2025-12-31' })
    await flushPromises()
    for (const label of ['全部', '当月', '当季', '当年', '去年']) {
      expect(lit(historical, label)).toBe(false)
    }
  })

  it('步进换算：< 从当月落上月（跨年回退 2026-01 → 2025-12），emit 上月快照、标签跟随', async () => {
    const wrapper = mountRange({ from: '2026-01-01', to: '2026-01-31' })
    await flushPromises()
    expect(periodLabel(wrapper)).toBe('2026年1月')
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(false)
    const range = await stepAndConsume(wrapper, 'prev')
    expect(range).toEqual({ from: '2025-12-01', to: '2025-12-31' })
    expect(periodLabel(wrapper)).toBe('2025年12月')
  })

  it('边界外步进置灰：最新期间 > 置灰；走到最早期间（2025-06）后 < 置灰、> 可走回；置灰点击不 emit', async () => {
    const wrapper = mountRange({ from: '2026-01-01', to: '2026-01-31' })
    await flushPromises()
    // 最新边界 = max(当前期间, 最新交易期间) = 2026-01 → 2026-02 不可达
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(true)
    // 连续 < 至最早期间 2025-06（7 步）
    let range: NullableDateRange = { from: '2026-01-01', to: '2026-01-31' }
    for (let i = 0; i < 7; i++) range = await stepAndConsume(wrapper, 'prev')
    expect(range).toEqual({ from: '2025-06-01', to: '2025-06-30' })
    expect(periodLabel(wrapper)).toBe('2025年6月')
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(true)
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(false)
    // 置灰按钮点击不产生新 emit
    const before = emitCount(wrapper)
    await stepButton(wrapper, 'prev').trigger('click')
    await flushPromises()
    expect(emitCount(wrapper)).toBe(before)
    // > 可走回
    await stepAndConsume(wrapper, 'next')
    expect(lastEmitted(wrapper)).toEqual({ from: '2025-07-01', to: '2025-07-31' })
  })

  it('退化：边界拉取失败时不钳制（> 可步进），不阻塞快捷选择', async () => {
    const failing: Promise<{ min_date: string | null; max_date: string | null }> =
      Promise.reject(new Error('boom'))
    failing.catch(() => {}) // 防 unhandled rejection 噪音
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'report_date_range') return failing
      if (cmd === 'list_insurers') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mountRange({ from: '2026-01-01', to: '2026-01-31' })
    await flushPromises()
    expect(stepButton(wrapper, 'next').props('disabled')).toBe(false)
  })

  it('面板：type 随当前游标单位切换（月/季/年），边界外月份置灰', async () => {
    const wrapper = mountRange({ from: '2026-01-01', to: '2026-01-31' })
    await flushPromises()
    const picker = wrapper.findComponent(NDatePicker)
    expect(wrapper.findComponent(AppDatePicker).exists()).toBe(true)
    expect(picker.props('type')).toBe('month')
    const isDisabled = picker.props('isDateDisabled') as (
      timestamp: number,
      detail: unknown,
    ) => boolean
    // 2025年5月 早于最早交易期间 → 置灰；2025年6月 界内可选
    expect(isDisabled(0, { type: 'month', year: 2025, month: 4 })).toBe(true)
    expect(isDisabled(0, { type: 'month', year: 2025, month: 5 })).toBe(false)
  })

  it('面板选择：点选边界内期间 emit 精确快照并关闭面板（弹层注册表同步撤销）', async () => {
    const wrapper = mountRange({ from: '2026-01-01', to: '2026-01-31' })
    await flushPromises()
    await wrapper.find('.period-label').trigger('click')
    await flushPromises()
    expect(hasOpenOverlay()).toBe(true)
    // 选 2025年12月（界内）：emit 自然月快照
    const picker = wrapper.findComponent(NDatePicker)
    picker.vm.$emit('update:value', new Date(2025, 11, 1).getTime())
    await flushPromises()
    expect(lastEmitted(wrapper)).toEqual({ from: '2025-12-01', to: '2025-12-31' })
    // 面板关闭 + 注册表撤销
    expect(hasOpenOverlay()).toBe(false)
  })

  it('键盘可达：期间标签聚焦后 Enter/Space 打开面板，aria-expanded 随开合（issue #425）', async () => {
    const wrapper = mountRange({ from: '2026-01-01', to: '2026-01-31' })
    await flushPromises()
    // 期间标签按钮：aria-haspopup 标记面板触发器，aria-expanded 初始收合
    const trigger = wrapper.find('[aria-haspopup="dialog"]')
    expect(trigger.exists()).toBe(true)
    expect(trigger.attributes('aria-expanded')).toBe('false')
    await trigger.trigger('keydown', { key: 'Enter' })
    await flushPromises()
    expect(hasOpenOverlay()).toBe(true)
    expect(trigger.attributes('aria-expanded')).toBe('true')
    // 关闭（update:show = false）→ aria-expanded 回落
    wrapper.findComponent(NDatePicker).vm.$emit('update:show', false)
    await flushPromises()
    expect(trigger.attributes('aria-expanded')).toBe('false')
    // Space 同样打开
    await trigger.trigger('keydown', { key: ' ' })
    await flushPromises()
    expect(hasOpenOverlay()).toBe(true)
  })

  it('面板开/关上报弹层注册表（Overlay Suppression 不回退）', async () => {
    const wrapper = mountRange()
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)
    // 点期间标签打开面板 → date-picker 上报打开
    await wrapper.find('.period-label').trigger('click')
    await flushPromises()
    expect(openOverlayNames()).toContain('date-picker')
    // 关闭（update:show = false，经封装双路上报）→ 注册表撤销
    wrapper.findComponent(NDatePicker).vm.$emit('update:show', false)
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)
  })

  it('presets prop 收窄芯片闭集（报表页日期闭集消费形态，ADR-0057）：无「全部」', async () => {
    const wrapper = mount(QuickTimeRange, {
      props: { modelValue: { from: '2026-01-01', to: '2026-12-31' } as NullableDateRange, presets: DATED_TIME_PERIOD_PRESETS },
    })
    await flushPromises()
    expect(chip(wrapper, '全部')).toBeUndefined()
    for (const label of ['当月', '当季', '当年', '去年']) {
      expect(chip(wrapper, label).exists()).toBe(true)
    }
  })

  it('数据期间边界失效重拉：ledger:changed 后即时外扩（钳制边界跟随新数据）', async () => {
    const mockListen = vi.mocked(listen)
    const handlers: Array<(evt: unknown) => void> = []
    mockListen.mockImplementation(async (_evt, handler) => {
      handlers.push(handler)
      return vi.fn()
    })
    // 单月数据：月档边界 [2026-01, 2026-01]，< 置灰
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'report_date_range')
        return Promise.resolve({ min_date: '2026-01-05', max_date: '2026-01-05' })
      if (cmd === 'list_insurers') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mountRange({ from: '2026-01-01', to: '2026-01-31' })
    await flushPromises()
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(true)
    // 数据外扩历史（AI 导入）→ ledger:changed 重拉 → < 随新边界（2025-08）解锁
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'report_date_range')
        return Promise.resolve({ min_date: '2025-08-01', max_date: '2026-01-05' })
      if (cmd === 'list_insurers') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    handlers.forEach((h) => h({ event: 'ledger:changed', payload: null }))
    await flushPromises()
    expect(stepButton(wrapper, 'prev').props('disabled')).toBe(false)
  })
})
