import { flushPromises } from '@vue/test-utils'
import type { VueWrapper } from '@vue/test-utils'

/**
 * 移动档导航壳测试助手（issue #842）：App 壳两档分支测试与 MobileNavShell
 * 组件测试共享的抽屉查询/操作单点（ADR-0085 决策 6 测试侧助手收口同款精神）。
 * 抽屉内容经 NDrawer 传送门到 body，一律从 document 查询。
 */

/** 点汉堡打开抽屉并等稳。 */
export async function openMobileDrawer(wrapper: VueWrapper) {
  await wrapper.find('.mobile-hamburger').trigger('click')
  await flushPromises()
}

/** 抽屉内菜单项文案（有序）。 */
export function drawerMenuItemTexts(): string[] {
  return [...document.body.querySelectorAll('.n-drawer .n-menu-item-content')].map(
    (el) => el.textContent ?? '',
  )
}

/** 抽屉内组标题行「更多」链接文案。 */
export function drawerMoreLinkTexts(): string[] {
  return [...document.body.querySelectorAll('.n-drawer .group-more-link')].map(
    (el) => el.textContent ?? '',
  )
}

/** 抽屉内组标题文案（剥掉「更多」链接文本）。 */
export function drawerGroupTitles(): string[] {
  return [...document.body.querySelectorAll('.n-drawer .sidebar-group-title')].map(
    (el) => (el.textContent ?? '').replace(/更多/g, '').trim(),
  )
}

/** 找抽屉内文案含 label 的菜单项元素。 */
export function findDrawerItem(label: string): HTMLElement {
  const item = drawerMenuItemTexts().length
    ? ([...document.body.querySelectorAll('.n-drawer .n-menu-item-content')].find(
        (el) => el.textContent?.includes(label),
      ) as HTMLElement | undefined)
    : undefined
  return item!
}
