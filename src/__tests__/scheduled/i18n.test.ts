import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import ScheduledView from '@/views/ScheduledView.vue'
import { routes } from '@/router'
import { applyLocale } from '@/i18n'
import { occurrenceStatusLabel, scheduledStatusLabel } from '@/utils/scheduled'
import { scheduledRecurrenceLabel, scheduledRecurrenceOptions } from '@/composables/useScheduledPlanList'
import type { SubscriptionSpendOverview } from '@/types'

/**
 * 定时计划域 i18n 行为测试（issue #349 / ADR-0049）：
 * 默认语言恒为 zh-CN（测试环境不初始化），此处显式切 en-US 验证英文渲染与
 * 领域标签函数输出，用例结束还原 zh-CN，不影响其他测试文件（vitest 文件级隔离）。
 */

const mockInvoke = vi.mocked(invoke)

// jsdom 无 canvas：与 SubscriptionSpendPanel.test.ts 同款桩，避免真实 chart.js 响应式重渲染报错
vi.mock('vue-chartjs', async () => {
  const { BarChartStub } = await import('../line-chart-stub')
  return { Bar: BarChartStub }
})

enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

const emptySpendOverview: SubscriptionSpendOverview = {
  native_currency: 'CNY',
  this_month_native_cents: 0,
  this_year_native_cents: 0,
  months: [],
  rows: [],
  projected_month_native_cents: 0,
  projected_year_native_cents: 0,
}

function baseInvoke() {
  mockInvoke.mockImplementation(((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve([])
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
    if (cmd === 'subscription_spend_overview') return Promise.resolve(emptySpendOverview)
    if (cmd === 'list_scheduled_transactions') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  }) as typeof invoke)
}

async function mountView() {
  const r = createRouter({ history: createMemoryHistory(), routes })
  await r.push('/scheduled')
  await r.isReady()
  const wrapper = mount(ScheduledView, { global: { plugins: [r] } })
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
})

describe('定时计划域 i18n（默认 zh-CN，切换 en-US 即时生效）', () => {
  it('视图默认中文：页签与订阅清单卡片为中文', async () => {
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('订阅')
    expect(wrapper.text()).toContain('订阅清单')
  })

  it('切换 en-US：页签与卡片标题即时变为英文，还原 zh-CN 后恢复中文', async () => {
    const wrapper = await mountView()
    await applyLocale('en-US')
    await nextTick()
    expect(wrapper.text()).toContain('Subscriptions')
    expect(wrapper.text()).toContain('Installments')
    expect(wrapper.text()).toContain('Scheduled Transfers')
    expect(wrapper.text()).toContain('Subscription List')
    expect(wrapper.text()).toContain('Actual Spend')

    // 还原默认语言，避免污染同文件后续用例
    await applyLocale('zh-CN')
    await nextTick()
    expect(wrapper.text()).toContain('订阅清单')
  })

  it('点击「分期」页签：英文下渲染 Installment List', async () => {
    const wrapper = await mountView()
    await applyLocale('en-US')
    await nextTick()
    await wrapper.findAll('.n-tabs-tab').find((tab) => tab.text() === 'Installments')!.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('Installment List')
    expect(wrapper.find('[data-testid="inst-create-open"]').exists()).toBe(true)
    await applyLocale('zh-CN')
  })

  it('状态标签函数随语言输出：en Active / Running，zh 还原后中文', async () => {
    expect(scheduledStatusLabel('active')).toBe('进行中')
    expect(occurrenceStatusLabel('processing')).toBe('执行中')
    expect(scheduledRecurrenceLabel('monthly', 2)).toBe('每2月')

    await applyLocale('en-US')
    expect(scheduledStatusLabel('active')).toBe('Active')
    expect(scheduledStatusLabel('completed')).toBe('Completed')
    expect(occurrenceStatusLabel('processing')).toBe('Running')
    expect(occurrenceStatusLabel('failed')).toBe('Failed')
    expect(scheduledRecurrenceLabel('monthly', 1)).toBe('Every month')
    expect(scheduledRecurrenceLabel('monthly', 2)).toBe('Every 2 months')
    expect(scheduledRecurrenceLabel('unknown', 1)).toBe('Every unknown')
    expect(scheduledRecurrenceOptions().map((o) => o.label)).toEqual([
      'Daily',
      'Weekly',
      'Monthly',
      'Yearly',
    ])

    await applyLocale('zh-CN')
    expect(scheduledStatusLabel('active')).toBe('进行中')
  })
})
