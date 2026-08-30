import { describe, it, expect, beforeEach } from 'vitest'
import {
  VIEW_STATE_KEYS,
  getSavedRouteName,
  saveRouteName,
  loadSidebarCollapsed,
  saveSidebarCollapsed,
  loadReportsGroupLevel,
  saveReportsGroupLevel,
  getSavedSidebarOrder,
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

describe('view-state sidebarOrder（issue #269：key 与读助手，解析归顺序模块）', () => {
  it('无记录时返回 null', () => {
    expect(getSavedSidebarOrder()).toBeNull()
  })

  it('返回已存原始值（数组不做解析，防御归顺序模块）', () => {
    localStorage.setItem(VIEW_STATE_KEYS.sidebarOrder, '["reports","transactions"]')
    expect(getSavedSidebarOrder()).toEqual(['reports', 'transactions'])
  })

  it('损坏的 JSON 回退 null', () => {
    localStorage.setItem(VIEW_STATE_KEYS.sidebarOrder, 'not-json{')
    expect(getSavedSidebarOrder()).toBeNull()
  })

  it('非数组原始值原样透传（由顺序解析整体回退默认序）', () => {
    localStorage.setItem(VIEW_STATE_KEYS.sidebarOrder, '123')
    expect(getSavedSidebarOrder()).toBe(123)
  })
})

describe('view-state reportsGroupLevel', () => {
  it('默认二级（level2）', () => {
    expect(loadReportsGroupLevel()).toBe('level2')
  })

  it('保存一级后读回', () => {
    saveReportsGroupLevel('level1')
    expect(loadReportsGroupLevel()).toBe('level1')
    expect(localStorage.getItem(VIEW_STATE_KEYS.reportsGroupLevel)).toBe('"level1"')
  })

  it('非法值回退 level2（防脏数据）', () => {
    localStorage.setItem(VIEW_STATE_KEYS.reportsGroupLevel, '"level3"')
    expect(loadReportsGroupLevel()).toBe('level2')
  })
})
