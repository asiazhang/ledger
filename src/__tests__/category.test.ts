import { describe, it, expect } from 'vitest'
import type { Category } from '@/types'
import {
  rootCategories,
  categoryChildren,
  categoryPath,
  buildCategoryTree,
  categoryRoot,
} from '@/utils/category-tree'

function makeCategory(overrides: Partial<Category> = {}): Category {
  return {
    id: '',
    name: '',
    kind: 'expense',
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...overrides,
  }
}

const food = makeCategory({ id: 'food', name: '餐饮', kind: 'expense', sort_order: 1 })
const lunch = makeCategory({ id: 'lunch', name: '午餐', kind: 'expense', parent_id: 'food', sort_order: 2 })
const dinner = makeCategory({ id: 'dinner', name: '晚餐', kind: 'expense', parent_id: 'food', sort_order: 1 })
const transport = makeCategory({ id: 'transport', name: '交通', kind: 'expense', sort_order: 0 })
const salary = makeCategory({ id: 'salary', name: '工资', kind: 'income' })

const categories: Category[] = [food, lunch, dinner, transport, salary]

describe('rootCategories', () => {
  it('返回 parent_id 为 null 的分类', () => {
    expect(rootCategories(categories)).toEqual([food, transport, salary])
  })

  it('所有元素 parent_id 均为 null', () => {
    for (const c of rootCategories(categories)) {
      expect(c.parent_id).toBeNull()
    }
  })

  it('空数组返回空数组', () => {
    expect(rootCategories([])).toEqual([])
  })
})

describe('categoryChildren', () => {
  it('返回指定父分类的子分类', () => {
    expect(categoryChildren(categories, 'food')).toEqual([lunch, dinner])
  })

  it('无子分类返回空数组', () => {
    expect(categoryChildren(categories, 'transport')).toEqual([])
  })
})

describe('categoryPath', () => {
  it('顶级分类返回单级名称', () => {
    expect(categoryPath(categories, 'transport')).toBe('交通')
  })

  it('二级分类返回 父 > 子', () => {
    expect(categoryPath(categories, 'lunch')).toBe('餐饮 > 午餐')
  })

  it('null 返回空串', () => {
    expect(categoryPath(categories, null)).toBe('')
  })

  it('不存在的 ID 返回空串', () => {
    expect(categoryPath(categories, 'nonexistent')).toBe('')
  })
})

describe('buildCategoryTree', () => {
  it('按 kind 过滤并构建树形结构，默认排序', () => {
    const result = buildCategoryTree(categories, { kind: 'expense' })
    expect(result).toHaveLength(2)

    expect(result[0].key).toBe('transport')
    expect(result[0].label).toBe('交通')
    expect(result[0].depth).toBe(0)
    expect(result[0].category).toBe(transport)
    expect(result[0].children).toBeUndefined()

    const foodNode = result[1]
    expect(foodNode.key).toBe('food')
    expect(foodNode.depth).toBe(0)
    expect(foodNode.children).toHaveLength(2)
    expect(foodNode.children![0].key).toBe('dinner')
    expect(foodNode.children![0].depth).toBe(1)
    expect(foodNode.children![1].key).toBe('lunch')
    expect(foodNode.children![1].depth).toBe(1)
  })

  it('不传 kind 返回全量分类', () => {
    const result = buildCategoryTree(categories)
    expect(result).toHaveLength(3)
  })

  it('sort:false 不排序', () => {
    const result = buildCategoryTree(categories, { kind: 'expense', sort: false })
    expect(result).toHaveLength(2)
    expect(result[0].key).toBe('food')
  })

  it('空数组返回空数组', () => {
    expect(buildCategoryTree([])).toEqual([])
  })
})

describe('categoryRoot', () => {
  it('二级分类上卷到顶级父分类', () => {
    const root = categoryRoot(categories, 'lunch')
    expect(root).toBe(food)
  })

  it('顶级分类返回自身', () => {
    const root = categoryRoot(categories, 'transport')
    expect(root).toBe(transport)
  })

  it('不存在的 ID 返回 undefined', () => {
    expect(categoryRoot(categories, 'nonexistent')).toBeUndefined()
  })
})
