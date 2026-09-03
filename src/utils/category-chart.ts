// 支出分类构成横向柱状图的数据形态（issue #378）：一级分类归并 + 未分类柱、
// 净额降序（负值柱如实沉底）、分类按 id 稳定配色（跨年份/跨数据顺序恒定）、
// 未分类固定灰。柱尾只标金额；占比收进 tooltip，「金额 · 占比%」标签在此收口为纯函数。
import type { Category, CategoryShare } from '@/types'
import { categoryRoot } from '@/utils/category-tree'
import { formatAmount } from '@/utils/money'

/** 未分类柱固定灰：与真实分类一眼区分 */
export const UNCATEGORIZED_COLOR = '#909399'

/** 分类色板（自环形图迁移）：命中顺序不参与取色，按 id 散列取色 */
const PALETTE = [
  '#5470c6', '#91cc75', '#fac858', '#ee6666', '#73c0de', '#3ba272',
  '#fc8452', '#9a60b4', '#ea7ccc', '#18a058', '#d03050', '#2080f0',
]

/** FNV-1a 32 位字符串散列：分类 id 稳定映射到色板下标 */
function hashId(id: string): number {
  let h = 0x811c9dc5
  for (let i = 0; i < id.length; i++) {
    h ^= id.charCodeAt(i)
    h = Math.imul(h, 0x01000193)
  }
  return h >>> 0
}

/** 分类颜色：同 id 恒同色（跨年份、跨数据顺序、跨层级） */
export function categoryColor(id: string): string {
  return PALETTE[hashId(id) % PALETTE.length]
}

/** 横向柱状图单根柱：一级归并（或未分类）后的图行 */
export interface CategoryBar {
  /** 根分类 id；null = 未分类 */
  id: string | null
  name: string
  /** 净额（分）：正值为净支出，负值为退款大于支出（如实渲染，0 轴） */
  value: number
  color: string
}

/** 归并结果收口：净额降序（负值沉底）+ 按 id 配色（未分类固定灰） */
function toBars(
  entries: Iterable<{ id: string | null; name: string; value: number }>,
): CategoryBar[] {
  return Array.from(entries)
    .sort((a, b) => b.value - a.value)
    .map((e) => ({ ...e, color: e.id === null ? UNCATEGORIZED_COLOR : categoryColor(e.id) }))
}

/**
 * 图行构建（基础态）：叶子份额按 `categoryRoot` 归并到一级根分类、未分类单列一柱；
 * 净额降序（负值沉底，同额保持后端序）；净额 0 的分类不进图。
 * 参考数据中已不存在的分类（软删等）叶子自成一行、按自身 id 配色。
 */
export function categoryBars(shares: CategoryShare[], categories: Category[]): CategoryBar[] {
  const merged = new Map<string | null, { id: string | null; name: string; value: number }>()
  for (const s of shares) {
    if (s.amount_cents === 0) continue
    const root = s.category_id ? categoryRoot(categories, s.category_id) : undefined
    const id = root ? root.id : s.category_id || null
    const name = root ? root.name : s.category_name
    const exist = merged.get(id)
    if (exist) exist.value += s.amount_cents
    else merged.set(id, { id, name, value: s.amount_cents })
  }
  return toBars(merged.values())
}

/**
 * 图行构建（图内下钻态，issue #379）：某一级分类的二级构成——
 * 二级子分类行 + 该一级自身直挂行（交易表单不限定叶子，直挂行补齐口径），
 * 各柱合计恒等于父柱金额；净额降序（负值沉底）、净额 0 的行不进图。
 * 直挂行沿用父分类 id（配色同父柱，同分类同色），名称由调用方传入
 * （本地化「直挂」标记，纯函数不接 i18n）；参考数据根链断裂的行不进任何下钻。
 */
export function categoryDrilldownBars(
  shares: CategoryShare[],
  categories: Category[],
  rootId: string,
  directName: string,
): CategoryBar[] {
  const merged = new Map<string, { id: string; name: string; value: number }>()
  for (const s of shares) {
    if (s.amount_cents === 0) continue
    const root = s.category_id ? categoryRoot(categories, s.category_id) : undefined
    if (root?.id !== rootId) continue
    const direct = s.category_id === rootId
    const id = direct ? rootId : (s.category_id as string)
    const name = direct ? directName : (categories.find((c) => c.id === id)?.name ?? s.category_name)
    const exist = merged.get(id)
    if (exist) exist.value += s.amount_cents
    else merged.set(id, { id, name, value: s.amount_cents })
  }
  return toBars(merged.values())
}

/** 全部一级柱净额合计（tooltip 占比的分母）：代数和，负柱如实冲减 */
export function categoryBarTotal(bars: { value: number }[]): number {
  return bars.reduce((sum, b) => sum + b.value, 0)
}

/** tooltip 标签「金额 · 占比%」；分母为 0（无柱或正负相抵）时只显示金额，不出现除零。 */
export function barTooltipLabel(value: number, total: number): string {
  if (total === 0) return formatAmount(value)
  return `${formatAmount(value)} · ${Math.round((value / total) * 100)}%`
}
