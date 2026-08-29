import { describe, it, expect, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import {
  mockDetails,
  makeDetail,
  makeOccurrence,
  makePlan,
  mockInvoke,
  mountView,
  setMockPlans,
  setup,
} from './common'

beforeEach(setup)

describe('SubscriptionsPane 期次详情弹窗入口（issue #205）', () => {
  it('点击「期次」打开通用期次详情弹窗', async () => {
    const plan = makePlan({ id: 'a1' })
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    await wrapper.find('[data-testid="op-detail-a1"]').trigger('click')
    await flushPromises()
    // 弹窗内容渲染（display-directive="if"，仅在打开时挂载；NModal teleport 到 body）
    expect(document.body.querySelector('[data-testid="occ-plan-note"]')).not.toBeNull()
  })

  it('弹窗内重试成功后清单刷新（changed 信号联动）', async () => {
    const plan = makePlan({ id: 'a1' })
    setMockPlans([plan])
    mockDetails.set(
      'a1',
      makeDetail(plan, [], [
        makeOccurrence({ id: 'f1', scheduled_date: '2026-02-01', status: 'failed' }),
      ]),
    )
    const wrapper = await mountView()
    await wrapper.find('[data-testid="op-detail-a1"]').trigger('click')
    await flushPromises()
    const retryBtn = document.body.querySelector(
      '[data-testid="occ-retry-f1"]',
    ) as HTMLButtonElement
    expect(retryBtn).not.toBeNull()
    retryBtn.click()
    await flushPromises()
    // changed 信号触发清单重拉（list_scheduled_transactions 再次调用）
    const loadCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_scheduled_transactions')
    expect(loadCalls.length).toBeGreaterThanOrEqual(2)
  })
})
