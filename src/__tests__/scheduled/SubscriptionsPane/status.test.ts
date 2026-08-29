import { describe, it, expect, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { NPopconfirm } from 'naive-ui'
import { mockDetails, makeDetail, makePlan, mockInvoke, mountView, setMockPlans, setup } from './common'

beforeEach(setup)

describe('SubscriptionsPane 状态操作（issue #159）', () => {
  it('进行中的订阅可暂停（走既有状态命令）', async () => {
    const plan = makePlan({ id: 'a1' })
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    await wrapper.find('[data-testid="op-pause-a1"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { id: string; input: { new_status: string } }).input.new_status === 'paused',
      ),
    ).toBe(true)
  })

  it('已暂停的订阅可恢复', async () => {
    const plan = makePlan({ id: 'p1', status: 'paused' })
    setMockPlans([plan])
    mockDetails.set('p1', makeDetail(plan, []))
    const wrapper = await mountView()
    await wrapper.find('[data-testid="filter-paused"]').trigger('click')
    await flushPromises()
    await wrapper.find('[data-testid="op-resume-p1"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { id: string; input: { new_status: string } }).input.new_status === 'active',
      ),
    ).toBe(true)
  })

  it('取消需二次确认（NPopconfirm），确认后走状态命令', async () => {
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
  })

  it('已取消的订阅不再提供状态操作', async () => {
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
