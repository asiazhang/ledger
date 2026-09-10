import { describe, it, expect } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { NDropdown, NModal } from 'naive-ui'
import { setFakeMedia } from '../helpers/media-mock'
import { mountView, rowMenu, rowMenuKeys } from './common'

/**
 * 触控交互轴（issue #843 / ADR-0088 决策 6，词汇表「输入轴」）：两轴对比组件测试。
 * 换档一律经媒体查询测试接缝（helpers/media-mock）：默认桌面指针态（hover +
 * fine）为指针轴基线；`hover: none + pointer: coarse` 为触控轴（Android 手机 /
 * 平板）。断言「看到什么、交互后发生什么」：
 * - 裸键监听：触控轴不注册（按键无弹窗）；指针轴行为不变；
 * - 记一笔下拉键位标注：触控轴退役（提示不存在的键位是误导）；指针轴照渲染；
 * - 交易行「⋯」：两轴同规常显（入口全平台一致），触控轴点开同一菜单；
 * - 金额全文：触控轴点按可达（气泡全文）；指针轴无点按行为（悬停面零变化）。
 */

function pressKey(key: string) {
  window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }))
}

type ViewWrapper = Awaited<ReturnType<typeof mountView>>

/** 记一笔分裂按钮的下拉（选项含 create kind key，与行菜单按 delete 区分）。 */
function createKindDropdown(wrapper: ViewWrapper) {
  return wrapper.findAllComponents(NDropdown).find((d) =>
    (d.props('options') as Array<{ key?: string }>).some((o) => o.key === 'expense'),
  )!
}

function createKindLabels(wrapper: ViewWrapper): string[] {
  return (createKindDropdown(wrapper).props('options') as Array<{ key?: string; label?: string }>)
    .filter((o) => o.key !== 'create-lending-divider')
    .map((o) => o.label ?? '')
}

describe('TransactionsView 触控交互轴（issue #843 两轴对比）', () => {
  it('触控轴：记一笔裸键不绑定监听（按键不开弹窗）', async () => {
    setFakeMedia({ hover: 'none', pointer: 'coarse' })
    const wrapper = await mountView()
    pressKey('a')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
  })

  it('指针轴：裸键行为不变（a 直达支出弹窗）', async () => {
    setFakeMedia({ hover: 'hover', pointer: 'fine' })
    const wrapper = await mountView()
    pressKey('a')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(true)
    expect(wrapper.findComponent(NModal).props('title')).toBe('记一笔 · 支出')
  })

  it('触控轴：记一笔下拉键位标注退役；指针轴照渲染（两轴对比）', async () => {
    setFakeMedia({ hover: 'none', pointer: 'coarse' })
    const touch = await mountView()
    expect(createKindLabels(touch)).toEqual([
      '支出', '收入', '转账', '买入', '卖出', '转换', '借出', '借入',
    ])

    setFakeMedia({ hover: 'hover', pointer: 'fine' })
    const pointer = await mountView()
    expect(createKindLabels(pointer)).toEqual([
      '支出 a', '收入 i', '转账 z', '买入 b', '卖出 s', '转换 c', '借出', '借入',
    ])
  })

  it('触控轴：「⋯」常显可点，点开与右键同一菜单（edit/refund/add-item/delete）', async () => {
    setFakeMedia({ hover: 'none', pointer: 'coarse' })
    const wrapper = await mountView()
    await wrapper.findAll('.row-actions-btn')[0].trigger('click')
    await flushPromises()
    expect(rowMenu(wrapper).props('show')).toBe(true)
    expect(rowMenuKeys(wrapper)).toEqual(['edit', 'refund', 'add-item', 'menu-divider', 'delete'])
  })

  it('触控轴：金额点按查看全文（点按气泡）；指针轴点击无气泡（行为不变）', async () => {
    setFakeMedia({ hover: 'none', pointer: 'coarse' })
    const touch = await mountView()
    await touch.find('.amount-cell').trigger('click')
    await flushPromises()
    const touchPopover = document.body.querySelector('.n-popover')
    expect(touchPopover).not.toBeNull()
    expect(touchPopover!.textContent).toContain(
      touch.find('.amount-cell').text(),
    )

    setFakeMedia({ hover: 'hover', pointer: 'fine' })
    const pointer = await mountView()
    await pointer.find('.amount-cell').trigger('click')
    await flushPromises()
    expect(document.body.querySelector('.n-popover')).toBeNull()
  })
})
