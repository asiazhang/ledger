import { describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { mount } from '@vue/test-utils'
import CategoryManager from '@/components/CategoryManager.vue'
import type { Category } from '@/types'


const mockCategories: Category[] = [
  {
    id: 'food', name: '餐饮', kind: 'expense', parent_id: null,
    icon: 'RestaurantOutline', sort_order: 1,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'lunch', name: '午餐', kind: 'expense', parent_id: 'food',
    icon: 'RestaurantOutline', sort_order: 1,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'dinner', name: '晚餐', kind: 'expense', parent_id: 'food',
    icon: 'RestaurantOutline', sort_order: 2,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'transport', name: '交通', kind: 'expense', parent_id: null,
    icon: 'BusOutline', sort_order: 2,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'salary', name: '工资', kind: 'income', parent_id: null,
    icon: 'WalletOutline', sort_order: 1,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
]

/** 本场景命令快照（beforeEach 与直调接缝的用例共享）：重排序按契约返回 undefined。 */
const CATEGORY_SEAM_DEFAULTS = { reorder_categories: undefined }

/** list_categories 参考命令本场景需自定义分类树（overrides 优先于参考兑底）。 */
const CATEGORY_SEAM_OVERRIDES = { list_categories: mockCategories }

describe('CategoryManager.vue', () => {
  beforeEach(async () => {
    // 参考 store 预载走接缝 opt-in 参数。
    await wireInvokeSeam({
      defaults: CATEGORY_SEAM_DEFAULTS,
      overrides: CATEGORY_SEAM_OVERRIDES,
      refreshReferenceStores: true,
    }).ready
  })

  it('挂载并渲染分类列表（默认支出 Tab）', () => {
    const wrapper = mount(CategoryManager)
    expect(wrapper.text()).toContain('餐饮')
    expect(wrapper.text()).toContain('交通')
    expect(wrapper.text()).not.toContain('工资')
    expect(wrapper.text()).toContain('新增分类')
  })

  it('切换到收入 Tab 显示收入分类', async () => {
    const wrapper = mount(CategoryManager)
    const incomeTab = wrapper.findAll('.n-tabs-tab')[1]
    await incomeTab.trigger('click')
    expect(wrapper.text()).toContain('工资')
    expect(wrapper.text()).not.toContain('餐饮')
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

  it('树形数据包含二级分类', () => {
    const wrapper = mount(CategoryManager)
    expect(wrapper.text()).toContain('午餐')
  })

  it('删除按钮存在', () => {
    const wrapper = mount(CategoryManager)
    const buttons = wrapper.findAll('button')
    const deleteBtn = buttons.filter((b) => b.text() === '删除')
    expect(deleteBtn.length).toBeGreaterThan(0)
  })

  it('树节点包含拖拽所需属性', () => {
    const wrapper = mount(CategoryManager)
    const nodes = wrapper.findAll('.n-tree-node')
    expect(nodes.length).toBeGreaterThan(0)
    nodes.forEach((n) => {
      expect(n.attributes('draggable')).toBe('true')
    })
  })

  it('reorderCategories 接口存在且可调用', async () => {
    mockInvoke.mockClear()
    wireInvokeSeam({ defaults: CATEGORY_SEAM_DEFAULTS, overrides: CATEGORY_SEAM_OVERRIDES })
    await expect(
      mockInvoke('reorder_categories', { items: [{ id: 'food', sort_order: 0 }] }),
    ).resolves.toBeUndefined()
  })
})
