import { describe, it, expect, beforeEach } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setActivePinia, createPinia } from 'pinia'
import { routes } from '@/router'
import { useSidebarOrderStore } from '@/stores/sidebar-order'

/** /policies 收纳重定向透传 focus（spec #704 / issue #706，词汇表「实体定位参数
 *  （focus 参数）」「落点尊重组内收纳」）：真实路由表同构 memory router 直打
 *  beforeEnter 守卫——来源列深链 /policies?focus=<id> 在收纳态经重定向落资产
 *  「更多」保单页签，focus 不丢、高亮不丢；主项态不重定向、query 原样保留。 */

function makeRouter() {
  return createRouter({ history: createMemoryHistory(), routes })
}

beforeEach(() => {
  setActivePinia(createPinia())
  localStorage.clear()
})

describe('/policies 路由守卫（issue #706 focus 透传）', () => {
  it('收纳态（出厂种子）：/policies?focus=x 重定向 assets-more 且 focus 透传', async () => {
    const router = makeRouter()
    await router.push('/policies?focus=pol-1')
    await router.isReady()
    expect(router.currentRoute.value.name).toBe('assets-more')
    expect(router.currentRoute.value.query).toEqual({ tab: 'policies', focus: 'pol-1' })
  })

  it('主项态（保单已移回侧栏）：独立路由渲染，query 原样保留', async () => {
    useSidebarOrderStore().applyMoveBackToSidebar('policies')
    const router = makeRouter()
    await router.push('/policies?focus=pol-1')
    await router.isReady()
    expect(router.currentRoute.value.name).toBe('policies')
    expect(router.currentRoute.value.query).toEqual({ focus: 'pol-1' })
  })
})
