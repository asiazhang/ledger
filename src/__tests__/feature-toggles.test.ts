import { describe, it, expect } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { mockInvoke } from '@ledger/test-support/invoke-mock'
import {
  CLOSABLE_FEATURES,
  NON_CLOSABLE_FEATURES,
  isClosableFeature,
  parseClosedFeatures,
  useFeatureToggleStore,
} from '@/stores/feature-toggles'
import {
  VIEW_STATE_KEYS,
  getSavedClosedFeatures,
  saveSidebarOrders,
  saveContainmentLists,
} from '@/utils/view-state'

// 功能开关状态基座接口测试（issue #1241 / ADR-0116 决策 2/6）。
// 本票只做状态与读写，不做任何 UI 与过滤（后续票消费本 store 的关闭集合）。
// 「重启」惯用法 = setActivePinia(createPinia())：store 首次实例化即启动读路径
//（读 view_state:closed_features 经解析防御），新 pinia = 新一次启动。

/** 重启：换新 pinia，下一次 useFeatureToggleStore() 即新一次启动读路径 */
function reboot() {
  setActivePinia(createPinia())
}

describe('可关功能闭集（ADR-0116 决策 2：开发者策展闭集，改清单 = 修订 ADR）', () => {
  it('可关九项 = 预算、报表、定时、商户、投资、物品、保单、实物资产、保司（与侧栏视图名同词）', () => {
    expect([...CLOSABLE_FEATURES]).toEqual([
      'budget',
      'reports',
      'scheduled',
      'merchants',
      'investments',
      'items',
      'policies',
      'physicalAssets',
      'insurers',
    ])
  })

  it('不可关六项 = 概览、交易、账户、搜索、AI 导入、设置（机制必需与定位必需两类理由）', () => {
    expect([...NON_CLOSABLE_FEATURES]).toEqual([
      'dashboard',
      'transactions',
      'accounts',
      'search',
      'ai',
      'settings',
    ])
  })

  it('两清单不相交、无重复：每项 id 恰好属于可关或不可关之一（闭集无空洞）', () => {
    const all = [...CLOSABLE_FEATURES, ...NON_CLOSABLE_FEATURES]
    expect(new Set(all).size).toBe(all.length)
    expect(all).toHaveLength(15)
  })
})

describe('isClosableFeature（闭集判定，ADR-0116 决策 2）', () => {
  it('可关九项为真', () => {
    for (const id of CLOSABLE_FEATURES) expect(isClosableFeature(id)).toBe(true)
  })

  it('不可关六项、未知 id 与非字符串一律为假', () => {
    for (const id of NON_CLOSABLE_FEATURES) expect(isClosableFeature(id)).toBe(false)
    expect(isClosableFeature('bogus')).toBe(false)
    expect(isClosableFeature('')).toBe(false)
    expect(isClosableFeature(123)).toBe(false)
    expect(isClosableFeature(null)).toBe(false)
    expect(isClosableFeature(undefined)).toBe(false)
    expect(isClosableFeature({ id: 'budget' })).toBe(false)
  })
})

describe('parseClosedFeatures 解析防御（比照侧栏顺序口径，ADR-0116 决策 6）', () => {
  it('无记录 / 非数组（null、标量、字符串、对象）整体回退默认：全开 = 空关闭集合', () => {
    for (const raw of [null, undefined, 42, 'budget', {}, { budget: true }, true]) {
      expect(parseClosedFeatures(raw)).toEqual([])
    }
  })

  it('合法数组保留（集合语义：输出归一为可关清单序，与存储顺序无关）', () => {
    expect(parseClosedFeatures(['budget', 'reports'])).toEqual(['budget', 'reports'])
    expect(parseClosedFeatures(['reports', 'budget'])).toEqual(['budget', 'reports'])
  })

  it.each([
    ['未知 id', ['budget', 'bogus', 'physical_assets'], ['budget']],
    ['含不可关项（概览/交易/账户/搜索/AI 导入/设置一律拒绝）', ['budget', 'dashboard', 'settings'], ['budget']],
    ['全为不可关项', [...NON_CLOSABLE_FEATURES], []],
    ['非字符串项', ['budget', 123, null, { id: 'reports' }, ['items']], ['budget']],
    ['重复项', ['budget', 'budget', 'reports', 'budget'], ['budget', 'reports']],
  ] as const)('%s：只保留合法可关项并去重', (_case, raw, expected) => {
    expect(parseClosedFeatures(raw)).toEqual(expected)
  })

  it('混合脏数据：解析出的关闭集合始终是合法可关项子集（去重、无不可关项、无未知 id）', () => {
    const closed = parseClosedFeatures([
      'investments',
      'dashboard',
      'investments',
      'bogus',
      7,
      'policies',
      'settings',
      null,
    ])
    expect(closed).toEqual(['investments', 'policies'])
    for (const id of closed) expect(isClosableFeature(id)).toBe(true)
  })
})

describe('store 读路径：默认态全开（issue #1241 验收判据 1）', () => {
  it('无记录启动：关闭集合为空，九项皆未关闭', () => {
    reboot()
    const store = useFeatureToggleStore()
    expect(store.closedFeatures).toEqual([])
    for (const id of CLOSABLE_FEATURES) expect(store.isFeatureClosed(id)).toBe(false)
  })

  it('存储为非数组脏形状启动：整体回退全开，不产生非法状态', () => {
    localStorage.setItem(VIEW_STATE_KEYS.closedFeatures, JSON.stringify({ investments: true }))
    reboot()
    const store = useFeatureToggleStore()
    expect(store.closedFeatures).toEqual([])
    for (const id of CLOSABLE_FEATURES) expect(store.isFeatureClosed(id)).toBe(false)
  })

  it('存储含未知 / 不可关 / 重复项启动：解析后只留合法可关项', () => {
    localStorage.setItem(
      VIEW_STATE_KEYS.closedFeatures,
      JSON.stringify(['investments', 'dashboard', 'investments', 'bogus']),
    )
    reboot()
    expect(useFeatureToggleStore().closedFeatures).toEqual(['investments'])
  })
})

describe('store 写路径：点选即写 + 跨启动保持（issue #1241 验收判据 2）', () => {
  it('关闭即写：内存态与 localStorage 同步，isFeatureClosed 立即为真', () => {
    reboot()
    const store = useFeatureToggleStore()
    store.setFeatureClosed('investments', true)
    expect(store.isFeatureClosed('investments')).toBe(true)
    expect(store.closedFeatures).toEqual(['investments'])
    expect(getSavedClosedFeatures()).toEqual(['investments'])
  })

  it('重新打开即从关闭集合移除；集合清空后删除存储记录（默认态 = 无记录）', () => {
    reboot()
    const store = useFeatureToggleStore()
    store.setFeatureClosed('investments', true)
    store.setFeatureClosed('investments', false)
    expect(store.closedFeatures).toEqual([])
    expect(localStorage.getItem(VIEW_STATE_KEYS.closedFeatures)).toBeNull()
  })

  it('跨启动保持：关闭两项后重启仍在关闭集合（无记录才回全开）', () => {
    reboot()
    useFeatureToggleStore().setFeatureClosed('investments', true)
    useFeatureToggleStore().setFeatureClosed('scheduled', true)
    reboot()
    const store = useFeatureToggleStore()
    expect(store.closedFeatures).toEqual(['scheduled', 'investments'])
    expect(store.isFeatureClosed('investments')).toBe(true)
    expect(store.isFeatureClosed('scheduled')).toBe(true)
    expect(store.isFeatureClosed('budget')).toBe(false)
  })

  it('同值重复设置 no-op：边界不变不产生额外存储写入', () => {
    reboot()
    const store = useFeatureToggleStore()
    store.setFeatureClosed('reports', true)
    const written = localStorage.getItem(VIEW_STATE_KEYS.closedFeatures)
    store.setFeatureClosed('reports', true)
    store.setFeatureClosed('budget', false)
    expect(localStorage.getItem(VIEW_STATE_KEYS.closedFeatures)).toBe(written)
  })

  it('不可关项与未知 id 写入一律 no-op：拒绝产生非法关闭状态', () => {
    reboot()
    const store = useFeatureToggleStore()
    store.setFeatureClosed('dashboard', true)
    store.setFeatureClosed('transactions', true)
    store.setFeatureClosed('bogus', true)
    store.setFeatureClosed(123, true)
    expect(store.closedFeatures).toEqual([])
    expect(localStorage.getItem(VIEW_STATE_KEYS.closedFeatures)).toBeNull()
    for (const id of NON_CLOSABLE_FEATURES) expect(store.isFeatureClosed(id)).toBe(false)
  })
})

describe('边界：本票路径零后端调用、不触碰收纳清单与侧栏顺序存储（issue #1241 验收判据 4）', () => {
  it('读 / 写关闭集合不调用任何后端命令', () => {
    reboot()
    const store = useFeatureToggleStore()
    store.setFeatureClosed('items', true)
    store.isFeatureClosed('items')
    store.setFeatureClosed('items', false)
    expect(mockInvoke).not.toHaveBeenCalled()
  })

  it('写关闭集合不改写侧栏顺序与收纳清单存储（关闭 != 改写收纳清单）', () => {
    const orders = {
      bookkeeping: ['transactions', 'accounts', 'budget'],
      assets: ['investments', 'items'],
      insights: ['reports', 'search'],
    }
    const containment = {
      bookkeeping: ['scheduled', 'merchants'],
      assets: ['policies', 'physicalAssets', 'insurers'],
      insights: [],
    }
    saveSidebarOrders(orders)
    saveContainmentLists(containment)
    reboot()
    const store = useFeatureToggleStore()
    store.setFeatureClosed('investments', true)
    store.setFeatureClosed('scheduled', true)
    expect(JSON.parse(localStorage.getItem(VIEW_STATE_KEYS.sidebarOrder) as string)).toEqual(orders)
    expect(JSON.parse(localStorage.getItem(VIEW_STATE_KEYS.sidebarContainment) as string)).toEqual(containment)
  })
})
