import type { Category, CategoryKind } from './index'

/** NTreeSelect 树形节点（key/label/children）。 */
export interface CategoryTreeNode {
  key: string
  label: string
  children?: CategoryTreeNode[]
  [key: string]: unknown
}

function makeCategoryMap(categories: Category[]): Map<string, Category> {
  const m = new Map<string, Category>()
  categories.forEach((c) => m.set(c.id, c))
  return m
}

export function rootCategories(categories: Category[]): Category[] {
  return categories.filter((c) => c.parent_id == null)
}

/** 取某父分类的直系子分类。 */
export function categoryChildren(categories: Category[], parentId: string): Category[] {
  return categories.filter((c) => c.parent_id === parentId)
}

/** 分类层级路径文本：二级显示"父 > 子"，顶级显示单级名；找不到返回空串。 */
export function categoryPath(categories: Category[], id: string | null | undefined): string {
  if (id == null) return ''
  const map = makeCategoryMap(categories)
  const cat = map.get(id)
  if (!cat) return ''
  if (cat.parent_id == null) return cat.name
  const parent = map.get(cat.parent_id)
  return parent ? `${parent.name} > ${cat.name}` : cat.name
}

/** 构造 NTreeSelect 树形 options：顶级可选中，有子分类的顶级展开二级。 */
export function treeCategoryOptions(categories: Category[], kind: CategoryKind): CategoryTreeNode[] {
  return rootCategories(categories)
    .filter((c) => c.kind === kind)
    .map((root) => {
      const children = categoryChildren(categories, root.id)
      const node: CategoryTreeNode = { key: root.id, label: root.name }
      if (children.length > 0) {
        node.children = children.map((c) => ({ key: c.id, label: c.name }))
      }
      return node
    })
}
