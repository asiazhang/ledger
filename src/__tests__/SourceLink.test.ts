import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import SourceLink from '@/components/SourceLink.vue'
import { useSidebarOrderStore } from '@/stores/sidebar-order'
import { setFakeMedia } from '@ledger/test-support/media-mock'
import type { TransactionSource } from '@ledger/types'

// 点击跳转经 useRouter（MerchantLink/AccountLink 同款 pushMock 断言先例）
const pushMock = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push: pushMock }),
}))

function makeSource(partial: Partial<TransactionSource> = {}): TransactionSource {
  return {
    kind: 'policy',
    entity_id: 'pol-1',
    display_name: '重疾险',
    status: null,
    ...partial,
  }
}

beforeEach(() => {
  pushMock.mockReset()
})

describe('SourceLink 来源列单元格（spec #704 / issue #706）', () => {
  it('渲染类型图标 + 实体名，悬停 tooltip 为来源类型全称', () => {
    const wrapper = mount(SourceLink, { props: { source: makeSource() } })
    expect(wrapper.find('.source-cell-icon').exists()).toBe(true)
    expect(wrapper.find('button.source-link').text()).toBe('重疾险')
    expect(wrapper.find('.source-cell').attributes('title')).toBe('保单')
  })

  it('点击经来源跳转深模块落地：收纳态（出厂保单在资产·更多）落「更多」保单页签 + focus', async () => {
    const wrapper = mount(SourceLink, {
      props: { source: makeSource({ entity_id: 'pol-9' }) },
    })
    await wrapper.find('button.source-link').trigger('click')
    expect(pushMock).toHaveBeenCalledTimes(1)
    expect(pushMock).toHaveBeenCalledWith({
      name: 'assets-more',
      query: { tab: 'policies', focus: 'pol-9' },
    })
  })

  it('主项态（保单已移回侧栏）落保单独立路由 + focus', async () => {
    useSidebarOrderStore().applyMoveBackToSidebar('policies')
    const wrapper = mount(SourceLink, { props: { source: makeSource() } })
    await wrapper.find('button.source-link').trigger('click')
    expect(pushMock).toHaveBeenCalledWith({
      name: 'policies',
      query: { focus: 'pol-1' },
    })
  })

  it('软删保单（status=deleted）：名称 +「已删除」标注，无按钮、点击不可达', () => {
    const wrapper = mount(SourceLink, {
      props: { source: makeSource({ status: 'deleted' }) },
    })
    expect(wrapper.find('button').exists()).toBe(false)
    expect(wrapper.find('.source-name').text()).toBe('重疾险')
    expect(wrapper.text()).toContain('已删除')
    // 名称仍带类型 tooltip（历史可见，只是不跳）
    expect(wrapper.find('.source-cell').attributes('title')).toBe('保单')
  })

  it('在册保单（status=null）无状态标注', () => {
    const wrapper = mount(SourceLink, { props: { source: makeSource({ status: null }) } })
    expect(wrapper.text()).not.toContain('已删除')
  })
})

describe('计划来源渲染（spec #704 / issue #707）：三形态图标 + 计划名，已取消可点击', () => {
  it.each([
    ['installmentPlan', '分期计划'],
    ['subscription', '订阅计划'],
    ['scheduledTransfer', '定时转账计划'],
  ] as const)('计划来源（%s）：类型图标 + 计划名，可点击', (kind, kindLabel) => {
    const wrapper = mount(SourceLink, {
      props: { source: makeSource({ kind, entity_id: 'plan-1', display_name: '视频会员' }) },
    })
    expect(wrapper.find('.source-cell-icon').exists()).toBe(true)
    expect(wrapper.find('button.source-link').text()).toBe('视频会员')
    expect(wrapper.find('.source-cell').attributes('title')).toBe(kindLabel)
  })

  it('已取消计划（status=cancelled）：名称 +「已取消」标注，仍可点击（不提供落空跳转的裁决只限软删保单）', async () => {
    const wrapper = mount(SourceLink, {
      props: { source: makeSource({ kind: 'subscription', status: 'cancelled' }) },
    })
    expect(wrapper.find('button.source-link').exists()).toBe(true)
    expect(wrapper.text()).toContain('已取消')
    await wrapper.find('button.source-link').trigger('click')
    expect(pushMock).toHaveBeenCalledWith({
      name: 'bookkeeping-more',
      query: { tab: 'scheduled', scheduledTab: 'subscriptions', focus: 'pol-1' },
    })
  })

  it('无备注计划（展示名空串）：按来源类型名兜底展示', () => {
    const wrapper = mount(SourceLink, {
      props: { source: makeSource({ kind: 'installmentPlan', display_name: '' }) },
    })
    expect(wrapper.find('button.source-link').text()).toBe('分期计划')
  })
})

describe('物品来源渲染（spec #704 / issue #708）：物品图标 + 物品名，已处置标注、可点击', () => {
  it('在用物品：类型图标 + 物品名，无状态标注，可点击', async () => {
    const wrapper = mount(SourceLink, {
      props: {
        source: makeSource({ kind: 'item', entity_id: 'item-1', display_name: '手机' }),
      },
    })
    expect(wrapper.find('.source-cell-icon').exists()).toBe(true)
    expect(wrapper.find('button.source-link').text()).toBe('手机')
    expect(wrapper.find('.source-cell').attributes('title')).toBe('物品')
    expect(wrapper.text()).not.toContain('已处置')
    await wrapper.find('button.source-link').trigger('click')
    expect(pushMock).toHaveBeenCalledWith({ name: 'items', query: { focus: 'item-1' } })
  })

  it('已处置物品（status=disposed）：名称 +「已处置」标注，仍可点击（物品列表带标注在册，跳转不落空）', async () => {
    const wrapper = mount(SourceLink, {
      props: {
        source: makeSource({
          kind: 'item',
          entity_id: 'item-2',
          display_name: '耳机',
          status: 'disposed',
        }),
      },
    })
    expect(wrapper.find('button.source-link').exists()).toBe(true)
    expect(wrapper.text()).toContain('已处置')
    await wrapper.find('button.source-link').trigger('click')
    expect(pushMock).toHaveBeenCalledWith({ name: 'items', query: { focus: 'item-2' } })
  })
})

describe('标的来源渲染（spec #704 / issue #709）：走势图标 + 代码与名称，可点击', () => {
  it('标的来源：类型图标 + 代码与名称展示名，无状态标注，主项态点击落投资主项路由 + focus', async () => {
    const wrapper = mount(SourceLink, {
      props: {
        source: makeSource({
          kind: 'instrument',
          entity_id: 'inst-1',
          display_name: '600519 招商银行',
        }),
      },
    })
    expect(wrapper.find('.source-cell-icon').exists()).toBe(true)
    expect(wrapper.find('.source-cell').attributes('title')).toBe('标的')
    expect(wrapper.find('button.source-link').text()).toBe('600519 招商银行')
    expect(wrapper.text()).not.toContain('已删除')
    await wrapper.find('button.source-link').trigger('click')
    expect(pushMock).toHaveBeenCalledWith({
      name: 'investments',
      query: { focus: 'inst-1' },
    })
  })

  it('收纳态（投资移入组「更多」）：点击落资产「更多」investments 页签 + focus', async () => {
    useSidebarOrderStore().applyMoveIntoMore('investments')
    const wrapper = mount(SourceLink, {
      props: {
        source: makeSource({ kind: 'instrument', entity_id: 'inst-2', display_name: '000001' }),
      },
    })
    await wrapper.find('button.source-link').trigger('click')
    expect(pushMock).toHaveBeenCalledWith({
      name: 'assets-more',
      query: { tab: 'investments', focus: 'inst-2' },
    })
  })
})

describe('来源列触控轴（issue #843 / ADR-0088 决策 6：类型全称常驻，title 不可达的一击可达替代）', () => {
  it('触控轴：类型全称常驻小字；名称可点击跳转不变；title 保留', async () => {
    setFakeMedia({ hover: 'none', pointer: 'coarse' })
    const wrapper = mount(SourceLink, {
      props: { source: makeSource({ kind: 'subscription', display_name: '视频会员' }) },
    })
    const label = wrapper.find('.source-type-label')
    expect(label.exists()).toBe(true)
    expect(label.text()).toBe('订阅计划')
    expect(wrapper.find('.source-cell').attributes('title')).toBe('订阅计划')
    await wrapper.find('button.source-link').trigger('click')
    expect(pushMock).toHaveBeenCalledTimes(1)
  })

  it('触控轴：展示名为类型名兜底（无备注计划）时不重复渲染类型小字', () => {
    setFakeMedia({ hover: 'none', pointer: 'coarse' })
    const wrapper = mount(SourceLink, {
      props: { source: makeSource({ kind: 'installmentPlan', display_name: '' }) },
    })
    expect(wrapper.find('.source-type-label').exists()).toBe(false)
  })

  it('指针轴：无常驻类型小字（行为不变，两轴对比）', () => {
    setFakeMedia({ hover: 'hover', pointer: 'fine' })
    const wrapper = mount(SourceLink, {
      props: { source: makeSource({ kind: 'subscription', display_name: '视频会员' }) },
    })
    expect(wrapper.find('.source-type-label').exists()).toBe(false)
    expect(wrapper.find('.source-cell').attributes('title')).toBe('订阅计划')
  })
})
