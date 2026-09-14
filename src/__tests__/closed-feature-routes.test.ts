import { describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { resolveRestoredViewName, restoreLastView, routes } from '@/router'
import {
  CLOSABLE_FEATURES,
  useFeatureToggleStore,
  type ClosableFeatureId,
} from '@/stores/feature-toggles'
import { groupOfView } from '@/stores/sidebar-order'
import { getSavedRouteName, saveRouteName } from '@ledger/utils/view-state'

/**
 * 关闭功能的裸路由守卫与上次视图回退（issue #1244 / ADR-0116 决策 4/5）。
 * 断言对准用户可观察结果——最终落在哪个视图（router.currentRoute）与启动落点，
 * 不对准线程或函数调用形状（ADR-0087）。
 */

function makeRouter(): Router {
  return createRouter({ history: createMemoryHistory(), routes })
}

/** 路由表按 name 取路径（不复制路由表；可关成员缺独立路由即红）。 */
function barePath(id: ClosableFeatureId): string {
  const record = routes.find((r) => r.name === id)
  if (!record || typeof record.path !== 'string') {
    throw new Error(`路由表缺少可关功能 ${id} 的独立路由`)
  }
  return record.path
}

async function landAt(path: string): Promise<Router> {
  const router = makeRouter()
  await router.push(path)
  await router.isReady()
  return router
}

function close(id: ClosableFeatureId): void {
  useFeatureToggleStore().setFeatureClosed(id, true)
}

function reopen(id: ClosableFeatureId): void {
  useFeatureToggleStore().setFeatureClosed(id, false)
}

/**
 * 打开态裸路由落点（出厂态）。注意两件事分开：出厂**收纳身份**（定时/商户/保单/
 * 实物资产/保司）与路由层**收纳分流**（只有保单/保司接线，先例 #360/#714）——定时/
 * 商户/实物资产的独立路由本就直达，「更多」页入口只是导航侧落点。
 */
const OPEN_BARE_LANDING: Record<ClosableFeatureId, { name: string; tab?: string }> = {
  budget: { name: 'budget' },
  reports: { name: 'reports' },
  scheduled: { name: 'scheduled' },
  merchants: { name: 'merchants' },
  investments: { name: 'investments' },
  items: { name: 'items' },
  policies: { name: 'assets-more', tab: 'policies' },
  physicalAssets: { name: 'physicalAssets' },
  insurers: { name: 'assets-more', tab: 'insurers' },
}

/** 组「更多」聚合页路由名（路由镜像侧栏层级，ADR-0063 决策 5）。 */
function groupMoreName(id: ClosableFeatureId): string {
  const gid = groupOfView(id)
  if (!gid) throw new Error(`可关功能 ${id} 无侧栏分组归属`)
  return `${gid}-more`
}

function groupMorePath(id: ClosableFeatureId): string {
  const record = routes.find((r) => r.name === groupMoreName(id))
  if (!record || typeof record.path !== 'string') throw new Error(`路由表缺少 ${id} 所在组的「更多」页`)
  return record.path
}

describe('关闭功能的裸路由守卫（issue #1244 / ADR-0116 决策 4/5）', () => {
  it.each([...CLOSABLE_FEATURES])('关闭「%s」后裸路由落概览', async (id) => {
    close(id)
    const router = await landAt(barePath(id))
    expect(router.currentRoute.value.name).toBe('dashboard')
  })

  it.each([...CLOSABLE_FEATURES])('打开态守卫放行（保单/保司按既有分流落资产·更多）', async (id) => {
    const expected = OPEN_BARE_LANDING[id]
    const router = await landAt(barePath(id))
    expect(router.currentRoute.value.name).toBe(expected.name)
    if (expected.tab) expect(router.currentRoute.value.query.tab).toBe(expected.tab)
  })

  it.each([...CLOSABLE_FEATURES])('关闭「%s」后带实体定位参数（focus）的深链放行直达', async (id) => {
    close(id)
    const router = await landAt(`${barePath(id)}?focus=entity-1`)
    expect(router.currentRoute.value.name).toBe(id)
    expect(router.currentRoute.value.query.focus).toBe('entity-1')
  })

  it.each([...CLOSABLE_FEATURES])('组「更多」页签指向已关闭功能（%s）：不再落无页签可落的组页', async (id) => {
    close(id)
    const router = await landAt(`${groupMorePath(id)}?tab=${id}`)
    expect(router.currentRoute.value.name).toBe('dashboard')
  })

  it('组「更多」引用深链指向已关闭功能：改落功能自有路由并保留 focus', async () => {
    close('policies')
    const router = await landAt(`${groupMorePath('policies')}?tab=policies&focus=pol-1`)
    expect(router.currentRoute.value.name).toBe('policies')
    expect(router.currentRoute.value.query.focus).toBe('pol-1')
  })

  it('组「更多」定时页签指向已关闭定时：容器形态页签归位到定时视图自己的 tab', async () => {
    close('scheduled')
    const router = await landAt(
      `${groupMorePath('scheduled')}?tab=scheduled&scheduledTab=installments&focus=plan-1`,
    )
    expect(router.currentRoute.value.name).toBe('scheduled')
    expect(router.currentRoute.value.query.tab).toBe('installments')
    expect(router.currentRoute.value.query.focus).toBe('plan-1')
  })

  it('组「更多」页签指向未关闭功能：原样落该组页（守卫不误伤）', async () => {
    const router = await landAt(`${groupMorePath('policies')}?tab=insurers`)
    expect(router.currentRoute.value.name).toBe('assets-more')
    expect(router.currentRoute.value.query.tab).toBe('insurers')
  })

  it('存量 /more 深链的页签指向已关闭功能：经迁移链也不落无页签可落的组页', async () => {
    close('policies')
    expect((await landAt('/more?tab=policies')).currentRoute.value.name).toBe('dashboard')
    expect((await landAt('/more?tab=policies&focus=pol-1')).currentRoute.value.name).toBe('policies')
  })

  it('关闭后重新打开：裸路由恢复直达（同会话两态往返）', async () => {
    close('investments')
    expect((await landAt('/investments')).currentRoute.value.name).toBe('dashboard')
    reopen('investments')
    expect((await landAt('/investments')).currentRoute.value.name).toBe('investments')
  })
})

describe('关闭「投资」后其他域的引用照常（issue #1244 / ADR-0116 决策 4）', () => {
  it.each([
    ['按 kind 筛选已有交易', '/transactions?kinds=sell'],
    ['持仓下钻（标的参数）', '/transactions?instrument=ins-1'],
    ['分类下钻', '/transactions?category=cat-1'],
    ['商户排行下钻', '/transactions?merchant=mch-1'],
    ['来源列账户链接', '/transactions?account=acc-1'],
  ])('引用仍可达：%s', async (_label, path) => {
    close('investments')
    expect((await landAt(path)).currentRoute.value.name).toBe('transactions')
  })
})

describe('上次视图回退（issue #1244 / ADR-0116 决策 5）', () => {
  it('上次视图为已关闭功能：启动落点解析回退概览（接线负向条目）', () => {
    close('reports')
    saveRouteName('reports')
    expect(resolveRestoredViewName(getSavedRouteName())).toBe('dashboard')
  })

  it('上次视图为已关闭功能：冷启动最终落概览，且不把该功能当落点', async () => {
    close('items')
    saveRouteName('items')
    const router = makeRouter()
    const replace = vi.spyOn(router, 'replace')
    await restoreLastView(router)
    expect(replace).not.toHaveBeenCalledWith({ name: 'items' })
    expect(router.currentRoute.value.name).toBe('dashboard')
  })

  it('上次视图未关闭：原样恢复（冷启动回上次视图）', async () => {
    saveRouteName('reports')
    const router = makeRouter()
    await restoreLastView(router)
    expect(router.currentRoute.value.name).toBe('reports')
  })

  it('重新打开后：上次视图原样恢复', async () => {
    close('reports')
    reopen('reports')
    saveRouteName('reports')
    const router = makeRouter()
    await restoreLastView(router)
    expect(router.currentRoute.value.name).toBe('reports')
  })

  it('存量旧视图名照旧（more 不回退概览，ADR-0063 决策 5）', async () => {
    saveRouteName('more')
    const router = makeRouter()
    await restoreLastView(router)
    expect(router.currentRoute.value.name).toBe('bookkeeping-more')
  })

  it('存量旧视图名指向已关闭功能（subscriptions → 定时）：冷启动落概览', async () => {
    close('scheduled')
    saveRouteName('subscriptions')
    const router = makeRouter()
    await restoreLastView(router)
    expect(router.currentRoute.value.name).toBe('dashboard')
  })
})
