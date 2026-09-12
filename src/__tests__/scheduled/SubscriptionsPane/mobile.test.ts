import { describe, it, expect, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { NDataTable } from 'naive-ui'
import { setFakeMedia } from '@ledger/test-support/media-mock'
import { mockInvoke } from '@ledger/test-support/invoke-mock'
import { formatAmount } from '@ledger/money'
import { refCurrencies } from '@ledger/test-support/reference-stubs'
import { makeOccurrence, makeSubscriptionPlan } from '../../factories'
import type { VueWrapper } from '@vue/test-utils'
import { mockDetails, makeDetail, mountView, setMockPlans, setup } from './common'

// 金额断言委托形态（issue #770）：期待值调同一 formatAmount 实现，格式规则唯一归属其专测
const cny = refCurrencies[0]

beforeEach(setup)

/**
 * 订阅页签移动档（issue #848 / ADR-0088 决策 11 票⑧，词汇表「窗口分级」）：
 * 列结构三分（备注/金额/操作），信息并入副行不丢失；生命周期操作一击可达
 * （可见按钮 + ≥48px 触控目标）；状态过滤与操作描述符行为不回归。桌面档
 * 十列一字不动（回归红线）。
 */

function tableOf(wrapper: VueWrapper) {
  return wrapper.findComponent(NDataTable)
}

/** 订阅夹具：active 计划 + 商户 + 一笔 pending 期次（下期扣款）。 */
function wireActivePlan() {
  const plan = makeSubscriptionPlan({ id: 'a1', note: '视频会员', amount_cents: 1500 }, 'mer-1')
  setMockPlans([plan])
  mockDetails.set(
    'a1',
    makeDetail(plan, [makeOccurrence({ id: 'o1', scheduled_date: '2026-03-01' })]),
  )
  return plan
}

describe('SubscriptionsPane 移动档（issue #848 / ADR-0088 决策 11 票⑧）', () => {
  it('移动档列结构三分（备注/金额/操作），桌面档十列一字不动', async () => {
    wireActivePlan()
    const desktop = await mountView()
    expect((tableOf(desktop).props('columns') as unknown[]).length).toBe(10)
    desktop.unmount()

    setFakeMedia({ width: 600 })
    const mobile = await mountView()
    const columns = tableOf(mobile).props('columns') as Array<{ key?: string }>
    expect(columns.map((c) => c.key)).toEqual(['note', 'amount', 'actions'])
  })

  it('移动档信息并入副行不丢失：状态/周期/开始日/商户/分类/账户/金额/下期扣款全部可读', async () => {
    setFakeMedia({ width: 600 })
    wireActivePlan()
    const wrapper = await mountView()
    const row = wrapper.find('.n-data-table-tbody .n-data-table-tr')
    const text = row.text()
    expect(text).toContain('视频会员') // 备注（标题行）
    expect(text).toContain('进行中') // 状态
    expect(text).toContain('每月') // 周期
    expect(text).toContain('2026-01-01') // 开始日
    expect(text).toContain('视频平台') // 商户
    expect(text).toContain('订阅服务') // 分类
    expect(text).toContain('招商银行') // 账户
    expect(text).toContain(formatAmount(1500, cny)) // 金额
    // 下期扣款锚点与桌面同 testid：日期与金额并存
    const next = wrapper.find('[data-testid="next-charge-a1"]')
    expect(next.exists()).toBe(true)
    expect(next.text()).toContain('2026-03-01')
    expect(next.text()).toContain(formatAmount(1500, cny))
  })

  it('移动档生命周期操作一击可达：暂停/取消为可见按钮且 ≥48px 触控目标，暂停走同一状态命令', async () => {
    setFakeMedia({ width: 600 })
    wireActivePlan()
    const wrapper = await mountView()
    for (const key of ['pause', 'cancel']) {
      const btn = wrapper.find(`[data-testid="op-${key}-a1"]`)
      expect(btn.exists(), `应存在可见的「${key}」按钮`).toBe(true)
      const el = btn.element as HTMLElement
      expect(el.style.minWidth).toBe('48px')
      expect(el.style.minHeight).toBe('48px')
    }
    await wrapper.find('[data-testid="op-pause-a1"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { input: { new_status: string } }).input.new_status === 'paused',
      ),
    ).toBe(true)
  })

  it('移动档暂停/恢复两侧可达：paused 行恢复按钮 ≥48px 且走同一状态命令', async () => {
    setFakeMedia({ width: 600 })
    const paused = makeSubscriptionPlan({ id: 'p1', note: '已暂停订阅', status: 'paused' })
    setMockPlans([paused])
    mockDetails.set('p1', makeDetail(paused, []))
    const wrapper = await mountView()
    // paused 行默认不在「进行中」过滤内，切过滤后恢复按钮可见
    await wrapper.find('[data-testid="filter-paused"]').trigger('click')
    await flushPromises()
    const resume = wrapper.find('[data-testid="op-resume-p1"]')
    expect(resume.exists()).toBe(true)
    const el = resume.element as HTMLElement
    expect(el.style.minWidth).toBe('48px')
    expect(el.style.minHeight).toBe('48px')
    await resume.trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { input: { new_status: string } }).input.new_status === 'active',
      ),
    ).toBe(true)
  })

  it('移动档状态过滤不回归：过滤芯片照常切捧行集', async () => {
    setFakeMedia({ width: 600 })
    wireActivePlan()
    const paused = makeSubscriptionPlan({ id: 'p1', note: '已暂停订阅', status: 'paused' })
    setMockPlans([makeSubscriptionPlan({ id: 'a1', note: '进行中订阅' }), paused])
    mockDetails.set('a1', makeDetail(makeSubscriptionPlan({ id: 'a1' }), []))
    mockDetails.set('p1', makeDetail(paused, []))
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('进行中订阅')
    expect(wrapper.text()).not.toContain('已暂停订阅')
    await wrapper.find('[data-testid="filter-paused"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已暂停订阅')
    expect(wrapper.text()).not.toContain('进行中订阅')
  })

  it('移动档编辑入口照常（订阅真差异描述符在移动档同样在详情动作之后）', async () => {
    setFakeMedia({ width: 600 })
    wireActivePlan()
    const wrapper = await mountView()
    const detail = wrapper.find('[data-testid="op-detail-a1"]')
    const edit = wrapper.find('[data-testid="op-edit-a1"]')
    expect(detail.exists()).toBe(true)
    expect(edit.exists()).toBe(true)
    // 纵排堆叠中编辑位于详情之后（DOM 序）
    expect(
      detail.element.compareDocumentPosition(edit.element) &
        (globalThis.Node.DOCUMENT_POSITION_FOLLOWING as number),
    ).toBeTruthy()
  })

  it('跨断点缩窗实时换列（十列 ⇄ 三列）', async () => {
    wireActivePlan()
    const wrapper = await mountView()
    expect((tableOf(wrapper).props('columns') as unknown[]).length).toBe(10)
    setFakeMedia({ width: 600 })
    await flushPromises()
    expect((tableOf(wrapper).props('columns') as unknown[]).length).toBe(3)
    setFakeMedia({ width: 1280 })
    await flushPromises()
    expect((tableOf(wrapper).props('columns') as unknown[]).length).toBe(10)
  })
})
