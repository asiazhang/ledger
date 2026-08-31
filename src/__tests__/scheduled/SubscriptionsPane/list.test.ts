/**
 * 订阅清单渲染冒烟（ADR-0041 迁移步 2 收缩）：清单加载/按形态过滤/状态过滤/
 * 详情扩展时序用例已由 ScheduledPlanList 模块接口测试承接
 * （useScheduledPlanList.test.ts，刷新版本号镜像法）；本文件只验适配器的
 * 列渲染接线（含 expandDetail 接线：下期扣款日期/占位/加载失败三态与商户列）。
 * 迁移与删除记录见对应提交信息。
 */
import { describe, it, expect, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import {
  mockDetails,
  makeDetail,
  makeOccurrence,
  makePlan,
  mountView,
  setMockPlans,
  setup,
} from './common'

beforeEach(setup)

describe('SubscriptionsPane 订阅清单渲染冒烟（编排用例见 useScheduledPlanList.test.ts）', () => {
  it('默认只显示进行中（active）的订阅（默认过滤归模块，此处验渲染）', async () => {
    setMockPlans([
      makePlan({ id: 'a1', note: '进行中订阅' }),
      makePlan({ id: 'p1', note: '已暂停订阅', status: 'paused' }),
      makePlan({ id: 'c1', note: '已取消订阅', status: 'cancelled' }),
    ])
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('进行中订阅')
    expect(wrapper.text()).not.toContain('已暂停订阅')
    expect(wrapper.text()).not.toContain('已取消订阅')
  })

  it('切换过滤查看已暂停 / 已取消', async () => {
    setMockPlans([
      makePlan({ id: 'a1', note: '进行中订阅' }),
      makePlan({ id: 'p1', note: '已暂停订阅', status: 'paused' }),
      makePlan({ id: 'c1', note: '已取消订阅', status: 'cancelled' }),
    ])
    const wrapper = await mountView()
    await wrapper.find('[data-testid="filter-paused"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已暂停订阅')
    expect(wrapper.text()).not.toContain('进行中订阅')

    await wrapper.find('[data-testid="filter-cancelled"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已取消订阅')
    expect(wrapper.text()).not.toContain('已暂停订阅')
  })

  it('只展示订阅计划，分期 / 定时转账不出现（按形态过滤归模块，此处验渲染）', async () => {
    const plans = [
      makePlan({ id: 'a1', note: '视频会员' }),
      makePlan({ id: 'i1', note: '某分期', kind: 'installment' }),
      makePlan({ id: 't1', note: '某定时转账', kind: 'scheduled_transfer' }),
    ]
    setMockPlans(plans)
    mockDetails.set('a1', makeDetail(plans[0], []))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('视频会员')
    expect(wrapper.text()).not.toContain('某分期')
    expect(wrapper.text()).not.toContain('某定时转账')
  })

  it('每行显示下期扣款日与金额（expandDetail 接线：取最早 pending 期次，选取逻辑归模块）', async () => {
    const plan = makePlan({ id: 'a1', amount_cents: 1500 })
    setMockPlans([plan])
    mockDetails.set(
      'a1',
      makeDetail(plan, [
        makeOccurrence({ id: 'o2', scheduled_date: '2026-04-01' }),
        makeOccurrence({ id: 'o1', scheduled_date: '2026-03-01' }),
      ]),
    )
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('2026-03-01')
    expect(wrapper.text()).toContain('¥15')
    expect(wrapper.text()).not.toContain('2026-04-01')
  })

  it('无 pending 期次（窗口外/已取消）时下期扣款显示 — 占位，不推算日期', async () => {
    const plan = makePlan({ id: 'a1' })
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    const cell = wrapper.find('[data-testid="next-charge-a1"]')
    expect(cell.text()).toBe('—')
    // 不推算日期：占位格里不出现任何日期形串
    expect(cell.text()).not.toMatch(/\d{4}-\d{2}-\d{2}/)
  })

  it('详情命令失败时显示加载失败，不与「无 pending」混淆', async () => {
    const plan = makePlan({ id: 'a1' })
    setMockPlans([plan])
    // 不注册 a1 的详情：get_scheduled_transaction_detail 将 reject
    const wrapper = await mountView()
    expect(wrapper.find('[data-testid="next-charge-a1"]').text()).toBe('加载失败')
  })

  it('金额与周期按原始币种与规则展示', async () => {
    const plan = makePlan({ id: 'a1', amount_cents: 9900, recurrence_interval: 3 })
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('¥99')
    expect(wrapper.text()).toContain('每3月')
  })
})

describe('SubscriptionsPane 商户列（issue #190）', () => {
  it('列表显示计划商户（merchantMap 派生，改名即时生效）', async () => {
    const plan = makePlan({ id: 'a1', note: '视频会员' }, 'mer-1')
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('视频平台')
  })

  it('无商户计划显示 — 占位', async () => {
    const plan = makePlan({ id: 'a1', note: '视频会员' })
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('视频会员')
    // 商户列占位：不出现商户名
    expect(wrapper.text()).not.toContain('视频平台')
  })
})
