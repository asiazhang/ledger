import { describe, it, expect } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import {
  mountView,
  openMenuOnRow,
  selectRowMenu,
} from './common'

// 交易弹窗族排版统一（issue #632，spec #630）：四个弹窗的卡片外观收敛为
// AppModal cardSize 单一声明——记账/退款/编辑归 md（480）、加入物品归 sm（420）；
// 显式 :bordered="false" 由 AppModal 默认无边框承担。断言只看组件可观察输出
// （卡片宽度样式与边框类），不深究 naive-ui 内部实现。

/** 取 body 上可见的卡片元素：display-directive="if" 下弹窗卡片仅在开启时挂载，
 * 视图自身无其他 NCard，可见卡片即当前弹窗。 */
function visibleModalCard(): HTMLElement {
  const cards = [...document.querySelectorAll<HTMLElement>('.n-card')].filter((el) => {
    let node: Element | null = el
    while (node && node !== document.body) {
      if ((node as HTMLElement).style.display === 'none') return false
      node = node.parentElement
    }
    return true
  })
  expect(cards, '当前应恰有一个可见弹窗卡片').toHaveLength(1)
  return cards[0]
}

/** 断言当前弹窗卡片：宽度档位 + 无边框（AppModal 默认，调用点不再显式声明）。 */
function expectModalCard(wrapper: VueWrapper, width: string) {
  const card = visibleModalCard()
  expect(card.style.width).toBe(width)
  expect(card.classList.contains('n-card--bordered')).toBe(false)
}

async function openCreateModal(wrapper: VueWrapper) {
  const btn = wrapper.findAll('button').find((b) => b.text().includes('记一笔'))!
  await btn.trigger('click')
  await flushPromises()
}

/** 退款/加入物品/编辑：右键第一行（默认库为 expense 行）经行菜单开启。 */
async function openRowModal(wrapper: VueWrapper, key: 'refund' | 'add-item' | 'edit') {
  await openMenuOnRow(wrapper, 0)
  await selectRowMenu(wrapper, key)
}

describe('TransactionsView 交易弹窗族排版统一（issue #632）', () => {
  it.each([
    ['create', '480px'],
    ['refund', '480px'],
    ['edit', '480px'],
    ['add-item', '420px'],
  ])('「%s」弹窗卡片宽度归对应档位且默认无边框', async (key, width) => {
    const wrapper = await mountView()
    if (key === 'create') {
      await openCreateModal(wrapper)
    } else {
      await openRowModal(wrapper, key)
    }
    expectModalCard(wrapper, width)
  })
})
