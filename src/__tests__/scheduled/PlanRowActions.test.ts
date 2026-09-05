import { describe, it, expect, vi } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { NPopconfirm } from 'naive-ui'
import PlanRowActions from '@/components/scheduled/PlanRowActions.vue'
import type { ScheduledPlanRowAction } from '@/composables/useScheduledPlanList'

/**
 * 共享行操作渲染组件冒烟（spec #520 接缝二）：确认分支、测试锚点、空占位各覆盖一次。
 * 不重复清单模块接口测试（useScheduledPlanList.test.ts）已覆盖的可用性矩阵——
 * 组件只是「描述符 → 按钮」的纯渲染，可用性由描述符自带，不在此重断言。
 */

function makeAction(over: Partial<ScheduledPlanRowAction> & { key: ScheduledPlanRowAction['key'] }): ScheduledPlanRowAction {
  return {
    label: '标签',
    available: true,
    confirm: null,
    run: vi.fn(),
    ...over,
  }
}

enableAutoUnmount(afterEach)

describe('PlanRowActions 共享行操作渲染组件', () => {
  it('锚点命名 op-${key}-${rowId}：可用的描述符渲染为按钮，锚点与既有测试断言兼容', () => {
    const run = vi.fn()
    const wrapper = mount(PlanRowActions, {
      props: {
        rowId: 'a1',
        actions: [
          makeAction({ key: 'detail', label: '期次', run }),
          makeAction({ key: 'pause', label: '暂停', run }),
        ],
      },
    })
    expect(wrapper.find('[data-testid="op-detail-a1"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="op-pause-a1"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('期次')
    expect(wrapper.text()).toContain('暂停')
    // 点击非确认动作 → run 接通
    wrapper.find('[data-testid="op-detail-a1"]').trigger('click')
    expect(run).toHaveBeenCalled()
  })

  it('确认分支：confirm 非空经 AppPopconfirm 二次确认，确认后走 run', async () => {
    const run = vi.fn()
    const wrapper = mount(PlanRowActions, {
      props: {
        rowId: 'a1',
        actions: [
          makeAction({ key: 'cancel', label: '取消', confirm: '确认取消？', run }),
        ],
      },
    })
    // 触发按钮包在 AppPopconfirm 内，点击弹出确认层
    await wrapper
      .findComponent(NPopconfirm)
      .find('[data-testid="op-cancel-a1"]')
      .trigger('click')
    await flushPromises()
    const positive = document.body.querySelector('.n-popconfirm .n-button--primary-type')
    expect(positive).not.toBeNull()
    ;(positive as HTMLButtonElement).click()
    await flushPromises()
    expect(run).toHaveBeenCalled()
  })

  it('空占位：全不可用（或空数组）渲染「—」', () => {
    const wrapper = mount(PlanRowActions, {
      props: {
        rowId: 'a1',
        actions: [makeAction({ key: 'cancel', available: false })],
      },
    })
    expect(wrapper.text()).toBe('—')
    expect(wrapper.find('[data-testid="op-cancel-a1"]').exists()).toBe(false)
  })
})
