import { describe, it, expect, beforeEach } from 'vitest'
import {
  VIEW_STATE_KEYS,
  getSavedRouteName,
  saveRouteName,
  loadSidebarCollapsed,
  saveSidebarCollapsed,
  getSavedSidebarOrder,
  saveSidebarOrders,
  clearSidebarOrder,
} from '@/utils/view-state'

beforeEach(() => {
  localStorage.clear()
})

describe('view-state route', () => {
  it('无记录时返回 null', () => {
    expect(getSavedRouteName()).toBeNull()
  })

  it('保存后能读回', () => {
    saveRouteName('reports')
    expect(getSavedRouteName()).toBe('reports')
    expect(localStorage.getItem(VIEW_STATE_KEYS.route)).toBe('"reports"')
  })

  it('损坏的 JSON 回退 null', () => {
    localStorage.setItem(VIEW_STATE_KEYS.route, 'not-json{')
    expect(getSavedRouteName()).toBeNull()
  })

  it('非字符串值回退 null（防脏数据）', () => {
    localStorage.setItem(VIEW_STATE_KEYS.route, '123')
    expect(getSavedRouteName()).toBeNull()
  })
})

describe('view-state sidebarCollapsed', () => {
  it('默认展开（false）', () => {
    expect(loadSidebarCollapsed()).toBe(false)
  })

  it('保存折叠后读回并写入 localStorage', () => {
    saveSidebarCollapsed(true)
    expect(loadSidebarCollapsed()).toBe(true)
    expect(localStorage.getItem(VIEW_STATE_KEYS.sidebarCollapsed)).toBe('true')
  })
})

describe('view-state sidebarOrder（issue #269/#359：key 与读助手，解析归顺序模块）', () => {
  it('无记录时返回 null', () => {
    expect(getSavedSidebarOrder()).toBeNull()
  })

  it('返回已存原始值（组内序对象不做解析，防御归顺序模块）', () => {
    const grouped = { bookkeeping: ['transactions'], insights: [] }
    localStorage.setItem(VIEW_STATE_KEYS.sidebarOrder, JSON.stringify(grouped))
    expect(getSavedSidebarOrder()).toEqual(grouped)
  })

  it('旧平铺数组原样透传（由顺序解析整体回退默认序，issue #359）', () => {
    localStorage.setItem(VIEW_STATE_KEYS.sidebarOrder, '["reports","transactions"]')
    expect(getSavedSidebarOrder()).toEqual(['reports', 'transactions'])
  })

  it('损坏的 JSON 回退 null', () => {
    localStorage.setItem(VIEW_STATE_KEYS.sidebarOrder, 'not-json{')
    expect(getSavedSidebarOrder()).toBeNull()
  })

  it('非对象原始值原样透传（由顺序解析整体回退默认序）', () => {
    localStorage.setItem(VIEW_STATE_KEYS.sidebarOrder, '123')
    expect(getSavedSidebarOrder()).toBe(123)
  })
})

describe('view-state sidebarOrder 写路径（issue #270/#359）', () => {
  it('saveSidebarOrders 写入组内序对象 JSON，getSavedSidebarOrder 读回原始值', () => {
    const orders = {
      bookkeeping: ['transactions', 'accounts', 'budget', 'scheduled'],
      assets: ['investments', 'items'],
      insights: ['search', 'reports'],
    } as const
    saveSidebarOrders(orders)
    expect(localStorage.getItem(VIEW_STATE_KEYS.sidebarOrder)).toBe(JSON.stringify(orders))
    expect(getSavedSidebarOrder()).toEqual(orders)
  })

  it('clearSidebarOrder 移除 key，回退无记录态', () => {
    saveSidebarOrders({ bookkeeping: ['transactions'], assets: [], insights: [] })
    clearSidebarOrder()
    expect(localStorage.getItem(VIEW_STATE_KEYS.sidebarOrder)).toBeNull()
    expect(getSavedSidebarOrder()).toBeNull()
  })

  it('clearSidebarOrder 对不存在的 key 不报错', () => {
    expect(() => clearSidebarOrder()).not.toThrow()
  })
})
