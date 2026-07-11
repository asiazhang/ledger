import { describe, it, expect } from 'vitest'
import type { Category } from '@/types'
import { rootCategories, categoryChildren, categoryPath, treeCategoryOptions } from '@/types/category'

function makeCategory(overrides: Partial<Category> = {}): Category {
  return {
    id: '',
    name: '',
    kind: 'expense',
    parent_id: null,
    icon: null,
    color: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...overrides,
  }
}

const food = makeCategory({ id: 'food', name: '餐饮', kind: 'expense' })
const lunch = makeCategory({ id: 'lunch', name: '午餐', kind: 'expense', parent_id: 'food' })
const dinner = makeCategory({ id: 'dinner', name: '晚餐', kind: 'expense', parent_id: 'food' })
const transport = makeCategory({ id: 'transport', name: '交通', kind: 'expense' })
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

describe('treeCategoryOptions', () => {
  it('按 kind 过滤并构建树形结构', () => {
    const result = treeCategoryOptions(categories, 'expense')
    expect(result).toHaveLength(2) // food, transport (salary is income)

    const foodNode = result.find((n) => n.key === 'food')
    expect(foodNode).toBeDefined()
    expect(foodNode!.label).toBe('餐饮')
    expect(foodNode!.children).toHaveLength(2)
    expect(foodNode!.children![0]).toEqual({ key: 'lunch', label: '午餐' })
    expect(foodNode!.children![1]).toEqual({ key: 'dinner', label: '晚餐' })

    const transportNode = result.find((n) => n.key === 'transport')
    expect(transportNode!.children).toBeUndefined()
  })

  it('income 分类只返回收入分类', () => {
    const result = treeCategoryOptions(categories, 'income')
    expect(result).toHaveLength(1)
    expect(result[0].key).toBe('salary')
  })

  it('空数组返回空数组', () => {
    expect(treeCategoryOptions([], 'expense')).toEqual([])
  })
})
