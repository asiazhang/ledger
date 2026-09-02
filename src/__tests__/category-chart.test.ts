import { describe, it, expect } from 'vitest'
import {
  UNCATEGORIZED_COLOR,
  categoryBars,
  categoryColor,
  categoryBarTotal,
  categoryDrilldownBars,
  barEndLabel,
} from '@/utils/category-chart'
import type { Category, CategoryShare } from '@/types'
import { makeCategory } from './factories'

const categories: Category[] = [
  makeCategory({ id: 'food', name: '餐饮' }),
  makeCategory({ id: 'food-snack', name: '零食', parent_id: 'food', sort_order: 1 }),
  makeCategory({ id: 'transport', name: '交通', sort_order: 2 }),
]

const share = (category_id: string, amount_cents: number, category_name = category_id): CategoryShare => ({
  category_id,
  category_name,
  amount_cents,
})

describe('categoryBars 一级归并（issue #378）', () => {
  it('二级金额并入根分类，行集合 = 一级根 + 未分类柱', () => {
    const bars = categoryBars(
      [share('food', 5000, '餐饮'), share('food-snack', 1000, '零食'), share('transport', 3000, '交通')],
      categories,
    )
    expect(bars.map((b) => [b.name, b.value])).toEqual([
      ['餐饮', 6000],
      ['交通', 3000],
    ])
  })

  it('未分类（category_id 空）单列一柱：id 为 null、名沿用后端行名', () => {
    const bars = categoryBars([share('food', 5000, '餐饮'), share('', 800, '未分类')], categories)
    expect(bars).toHaveLength(2)
    const uncategorized = bars.find((b) => b.id === null)
    expect(uncategorized?.name).toBe('未分类')
    expect(uncategorized?.value).toBe(800)
  })

  it('深两级分类归并到最顶根（沿 parent_id 上溯）', () => {
    const deep = [makeCategory({ id: 'food-dessert', name: '甜品', parent_id: 'food-snack' })]
    const bars = categoryBars([share('food-dessert', 700, '甜品')], [...categories, ...deep])
    expect(bars.map((b) => [b.name, b.value])).toEqual([['餐饮', 700]])
  })

  it('净额 0 的分类不进图', () => {
    const bars = categoryBars([share('food', 0, '餐饮'), share('transport', 3000, '交通')], categories)
    expect(bars.map((b) => b.name)).toEqual(['交通'])
  })

  it('分类已删除（参考数据无行）时叶子自成一行、按自身 id 配色', () => {
    const bars = categoryBars([share('gone', 1200, '已删分类')], categories)
    expect(bars).toHaveLength(1)
    expect(bars[0].name).toBe('已删分类')
    expect(bars[0].id).toBe('gone')
    expect(bars[0].color).toBe(categoryColor('gone'))
    expect(bars[0].color).not.toBe(UNCATEGORIZED_COLOR)
  })
})

describe('categoryBars 排序与配色（issue #378）', () => {
  it('净额降序，负值柱如实沉底', () => {
    const bars = categoryBars(
      [share('transport', 3000, '交通'), share('negative', -500, '退款超支'), share('food', 6000, '餐饮')],
      categories,
    )
    expect(bars.map((b) => b.value)).toEqual([6000, 3000, -500])
  })

  it('同分类 id 颜色稳定：跨调用、跨数据顺序一致', () => {
    const a = categoryBars([share('food', 100, '餐饮'), share('transport', 200, '交通')], categories)
    const b = categoryBars([share('transport', 200, '交通'), share('food', 100, '餐饮')], categories)
    expect(a.find((x) => x.id === 'food')?.color).toBe(b.find((x) => x.id === 'food')?.color)
    expect(a.find((x) => x.id === 'transport')?.color).toBe(b.find((x) => x.id === 'transport')?.color)
  })

  it('未分类固定灰，不随 id 变', () => {
    const bars = categoryBars([share('', 800, '未分类')], categories)
    expect(bars[0].color).toBe(UNCATEGORIZED_COLOR)
  })
})

describe('categoryDrilldownBars 图内下钻（issue #379）', () => {
  const drillCategories: Category[] = [
    makeCategory({ id: 'food', name: '餐饮' }),
    makeCategory({ id: 'food-snack', name: '零食', parent_id: 'food', sort_order: 1 }),
    makeCategory({ id: 'food-coffee', name: '咖啡', parent_id: 'food', sort_order: 2 }),
    makeCategory({ id: 'transport', name: '交通', sort_order: 3 }),
  ]

  it('行集合 = 直挂行 + 二级子分类行，合计恒等于父柱金额', () => {
    const shares = [
      share('food', 2000, '餐饮'),
      share('food-snack', 1000, '零食'),
      share('transport', 3000, '交通'),
    ]
    const bars = categoryDrilldownBars(shares, drillCategories, 'food', '餐饮（直挂）')
    expect(bars.map((b) => [b.name, b.value])).toEqual([
      ['餐饮（直挂）', 2000],
      ['零食', 1000],
    ])
    // 合计 = 父柱金额（基础态餐饮柱 = 2000 + 1000 = 3000）
    expect(categoryBarTotal(bars)).toBe(
      categoryBarTotal(categoryBars(shares, drillCategories).filter((b) => b.id === 'food')),
    )
  })

  it('直挂行沿用父分类 id 与配色（同分类同色），二级行按自身 id 配色', () => {
    const bars = categoryDrilldownBars(
      [share('food', 2000, '餐饮'), share('food-snack', 1000, '零食')],
      drillCategories,
      'food',
      '餐饮（直挂）',
    )
    const direct = bars.find((b) => b.name === '餐饮（直挂）')
    expect(direct?.id).toBe('food')
    expect(direct?.color).toBe(categoryColor('food'))
    const snack = bars.find((b) => b.id === 'food-snack')
    expect(snack?.color).toBe(categoryColor('food-snack'))
    expect(snack?.color).not.toBe(direct?.color)
  })

  it('退款负值柱如实沉底，代数和仍等于父柱金额', () => {
    const bars = categoryDrilldownBars(
      [
        share('food', 5000, '餐饮'),
        share('food-snack', 8000, '零食'),
        share('food-coffee', -3000, '咖啡'),
      ],
      drillCategories,
      'food',
      '餐饮（直挂）',
    )
    expect(bars.map((b) => [b.name, b.value])).toEqual([
      ['零食', 8000],
      ['餐饮（直挂）', 5000],
      ['咖啡', -3000],
    ])
    expect(categoryBarTotal(bars)).toBe(10000)
  })

  it('非本根份额与未分类不进下钻图；净额 0 行不进图', () => {
    const bars = categoryDrilldownBars(
      [
        share('food', 2000, '餐饮'),
        share('transport', 3000, '交通'),
        share('', 800, '未分类'),
        share('food-coffee', 0, '咖啡'),
      ],
      drillCategories,
      'food',
      '餐饮（直挂）',
    )
    expect(bars.map((b) => b.name)).toEqual(['餐饮（直挂）'])
  })

  it('参考数据已不存在的分类（根链断裂）不进任何下钻', () => {
    const bars = categoryDrilldownBars(
      [share('gone', 1200, '已删分类'), share('food', 2000, '餐饮')],
      drillCategories,
      'food',
      '餐饮（直挂）',
    )
    expect(bars.map((b) => b.name)).toEqual(['餐饮（直挂）'])
  })
})

describe('柱尾标签与合计（issue #378）', () => {
  it('合计为全部一级柱净额的代数和（负柱冲减）', () => {
    expect(categoryBarTotal([{ value: 6000 }, { value: 3000 }, { value: -500 }])).toBe(8500)
    expect(categoryBarTotal([])).toBe(0)
  })

  it('柱尾标签 =「金额 · 占比%」，分母为全部一级柱合计', () => {
    // formatAmount: 8500 分 → "85"、6000 分 → "60"
    expect(barEndLabel(6000, 8500)).toBe('60 · 71%')
  })

  it('负柱显示负占比（净额口径诚实可查）', () => {
    expect(barEndLabel(-500, 8500)).toBe('-5 · -6%')
  })

  it('合计为 0（无柱或正负相抵）时只显示金额，不出现除零', () => {
    expect(barEndLabel(1200, 0)).toBe('12')
    expect(barEndLabel(-1200, 0)).toBe('-12')
  })
})
