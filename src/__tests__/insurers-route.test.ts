import { describe, it, expect, beforeEach } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setActivePinia, createPinia } from 'pinia'
import { routes } from '@/router'
import { useSidebarOrderStore } from '@/stores/sidebar-order'

/** /insurers 收纳分流守卫（issue #714，/policies 守卫先例）：保司管理出厂为
 *  资产组「更多」收纳成员——独立路由在收纳态重定向到资产·更多保司页签；
 *  用户「移回侧栏」后以主项身份在册，独立路由渲染保司管理页。 */

function makeRouter() {
  return createRouter({ history: createMemoryHistory(), routes })
}

beforeEach(() => {
  setActivePinia(createPinia())
  localStorage.clear()
})

describe('/insurers 路由守卫（issue #714）', () => {
  it('收纳态（出厂种子）：/insurers 重定向 assets-more 且带保司页签', async () => {
    const router = makeRouter()
    await router.push('/insurers')
    await router.isReady()
    expect(router.currentRoute.value.name).toBe('assets-more')
    expect(router.currentRoute.value.query).toEqual({ tab: 'insurers' })
  })

  it('主项态（保司已移回侧栏）：独立路由渲染，不重定向', async () => {
    useSidebarOrderStore().applyMoveBackToSidebar('insurers')
    const router = makeRouter()
    await router.push('/insurers')
    await router.isReady()
    expect(router.currentRoute.value.name).toBe('insurers')
  })
})
