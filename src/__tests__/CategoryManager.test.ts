import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '@/stores/app'
import CategoryManager from '@/components/CategoryManager.vue'
import type { Category } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCategories: Category[] = [
  {
    id: 'food', name: '餐饮', kind: 'expense', parent_id: null,
    icon: '🍜', color: '#FF6B6B', sort_order: 1,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'lunch', name: '午餐', kind: 'expense', parent_id: 'food',
    icon: null, color: null, sort_order: 1,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'dinner', name: '晚餐', kind: 'expense', parent_id: 'food',
    icon: null, color: null, sort_order: 2,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'transport', name: '交通', kind: 'expense', parent_id: null,
    icon: '🚌', color: '#4ECDC4', sort_order: 2,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'salary', name: '工资', kind: 'income', parent_id: null,
    icon: '💰', color: '#00B894', sort_order: 1,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
]

describe('CategoryManager.vue', () => {
  beforeEach(async () => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve([])
      if (cmd === 'list_accounts') return Promise.resolve([])
      if (cmd === 'list_categories') return Promise.resolve(mockCategories)
      if (cmd === 'reorder_categories') return Promise.resolve(undefined)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const store = useAppStore()
    await store.loadAll()
  })

  it('挂载并渲染分类列表', () => {
    const wrapper = mount(CategoryManager)
    expect(wrapper.text()).toContain('餐饮')
    expect(wrapper.text()).toContain('交通')
    expect(wrapper.text()).toContain('工资')
    expect(wrapper.text()).toContain('新增分类')
  })

  it('添加分类时校验空名称', async () => {
    mockInvoke.mockClear()
    const wrapper = mount(CategoryManager)
    const buttons = wrapper.findAll('button')
    const addBtn = buttons.filter((b) => b.text() === '添加')
    expect(addBtn.length).toBeGreaterThan(0)
    await addBtn[0].trigger('click')
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'create_category')
    expect(calls).toHaveLength(0)
  })

  it('展开的树形数据包含二级分类', () => {
    const wrapper = mount(CategoryManager)
    expect(wrapper.text()).toContain('午餐')
  })

  it('删除按钮存在并响应点击', async () => {
    const wrapper = mount(CategoryManager)
    const buttons = wrapper.findAll('button')
    const deleteBtn = buttons.filter((b) => b.text() === '删除')
    expect(deleteBtn.length).toBeGreaterThan(0)
  })

  it('拖拽手柄列存在', () => {
    const wrapper = mount(CategoryManager)
    expect(wrapper.html()).toContain('☰')
  })

  async function simulateDrag(
    wrapper: ReturnType<typeof mount>,
    fromId: string,
    toId: string,
  ) {
    const fromRow = wrapper.find(`tr[data-category-id="${fromId}"]`)
    const toRow = wrapper.find(`tr[data-category-id="${toId}"]`)
    expect(fromRow.exists()).toBe(true)
    expect(toRow.exists()).toBe(true)
    const dataTransfer = { setData: vi.fn(), effectAllowed: '', dropEffect: '' }
    await fromRow.trigger('dragstart', { dataTransfer })
    await toRow.trigger('dragover', { dataTransfer })
    await toRow.trigger('drop', { dataTransfer })
    await fromRow.trigger('dragend', { dataTransfer })
    await new Promise((resolve) => setTimeout(resolve))
  }

  it('同级根分类拖拽后调用 reorderCategories 且参数正确', async () => {
    mockInvoke.mockClear()
    const wrapper = mount(CategoryManager)
    await simulateDrag(wrapper, 'transport', 'food')

    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'reorder_categories')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toEqual({
      items: [
        { id: 'transport', sort_order: 0 },
        { id: 'food', sort_order: 1 },
      ],
    })
  })

  it('跨层拖拽不触发 reorderCategories', async () => {
    mockInvoke.mockClear()
    const wrapper = mount(CategoryManager)
    await simulateDrag(wrapper, 'food', 'lunch')

    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'reorder_categories')
    expect(calls).toHaveLength(0)
  })

  it('同级子分类拖拽调用 reorderCategories', async () => {
    mockInvoke.mockClear()
    const wrapper = mount(CategoryManager)
    await simulateDrag(wrapper, 'dinner', 'lunch')

    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'reorder_categories')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toEqual({
      items: [
        { id: 'dinner', sort_order: 0 },
        { id: 'lunch', sort_order: 1 },
      ],
    })
  })
})
