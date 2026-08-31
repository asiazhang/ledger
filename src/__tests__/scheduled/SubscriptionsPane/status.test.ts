import { describe, it, expect, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { NPopconfirm } from 'naive-ui'
import { mockDetails, makeDetail, makePlan, mockInvoke, mountView, setMockPlans, setup } from './common'

beforeEach(setup)

/**
 * 订阅页签操作列（ADR-0041 迁移步 2 收缩）：生命周期状态机（命令参数/成功提示/
 * 重拉时序/可用性矩阵）已由 ScheduledPlanList 模块接口测试承接
 * （useScheduledPlanList.test.ts，转账步先行落地）；此处只留交互冒烟
 * （描述符 → 按钮渲染与 onClick 接线）与订阅真差异——生命周期变更后订阅花费
 * 面板刷新（onStatusChanged 钩子接线，验收项）。迁移与删除记录见对应提交信息。
 */

/** 订阅花费总览拉取次数（花费面板挂载即拉 1 次，钩子刷新后递增）。 */
function spendCallCount() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'subscription_spend_overview').length
}

describe('SubscriptionsPane 操作列交互冒烟（状态机见 useScheduledPlanList.test.ts）', () => {
  it('active 行点「暂停」发出状态命令，且订阅花费面板随之刷新（onStatusChanged 钩子）', async () => {
    const plan = makePlan({ id: 'a1' })
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    const before = spendCallCount()
    await wrapper.find('[data-testid="op-pause-a1"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { id: string; input: { new_status: string } }).input.new_status === 'paused',
      ),
    ).toBe(true)
    expect(spendCallCount()).toBeGreaterThan(before)
  })

  it('已暂停的订阅可恢复，恢复后花费面板再次刷新', async () => {
    const plan = makePlan({ id: 'p1', status: 'paused' })
    setMockPlans([plan])
    mockDetails.set('p1', makeDetail(plan, []))
    const wrapper = await mountView()
    await wrapper.find('[data-testid="filter-paused"]').trigger('click')
    await flushPromises()
    const before = spendCallCount()
    await wrapper.find('[data-testid="op-resume-p1"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { id: string; input: { new_status: string } }).input.new_status === 'active',
      ),
    ).toBe(true)
    expect(spendCallCount()).toBeGreaterThan(before)
  })

  it('取消需二次确认（NPopconfirm），确认后走状态命令并刷新花费面板', async () => {
    const plan = makePlan({ id: 'a1' })
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    // 打开 Popconfirm
    await wrapper
      .findComponent(NPopconfirm)
      .find('[data-testid="op-cancel-a1"]')
      .trigger('click')
    await flushPromises()
    const positive = document.body.querySelector('.n-popconfirm .n-button--primary-type')
    expect(positive).not.toBeNull()
    const before = spendCallCount()
    ;(positive as HTMLButtonElement).click()
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { id: string; input: { new_status: string } }).input.new_status ===
            'cancelled',
      ),
    ).toBe(true)
    expect(spendCallCount()).toBeGreaterThan(before)
  })

  it('已取消的订阅不再提供状态操作（可用性矩阵归模块，此处验渲染接线）', async () => {
    const plan = makePlan({ id: 'c1', status: 'cancelled', note: '已取消订阅' })
    setMockPlans([plan])
    mockDetails.set('c1', makeDetail(plan, []))
    const wrapper = await mountView()
    await wrapper.find('[data-testid="filter-cancelled"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已取消订阅')
    expect(wrapper.find('[data-testid="op-pause-c1"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="op-resume-c1"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="op-cancel-c1"]').exists()).toBe(false)
  })
})
