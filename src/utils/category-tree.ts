import type { Category, CategoryKind } from '@/types'

export interface CategoryTreeNode {
  key: string
  label: string
  category: Category
  depth: number
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

export function categoryChildren(categories: Category[], parentId: string): Category[] {
  return categories.filter((c) => c.parent_id === parentId)
}

export function categoryPath(categories: Category[], id: string | null | undefined): string {
  if (id == null) return ''
  const map = makeCategoryMap(categories)
  const cat = map.get(id)
  if (!cat) return ''
  if (cat.parent_id == null) return cat.name
  const parent = map.get(cat.parent_id)
  return parent ? `${parent.name} > ${cat.name}` : cat.name
}

export function categoryRoot(categories: Category[], categoryId: string): Category | undefined {
  const map = makeCategoryMap(categories)
  let cat = map.get(categoryId)
  if (!cat) return undefined
  while (cat.parent_id != null) {
    const parent = map.get(cat.parent_id)
    if (!parent) break
    cat = parent
  }
  return cat
}

export function buildCategoryTree(
  categories: Category[],
  options?: { kind?: CategoryKind; sort?: boolean },
): CategoryTreeNode[] {
  const { kind, sort = true } = options ?? {}
  const filtered = kind != null
    ? categories.filter((c) => c.kind === kind)
    : categories

  let roots = filtered.filter((c) => c.parent_id == null)
  if (sort) roots = [...roots].sort((a, b) => a.sort_order - b.sort_order)

  return roots.map((root) => {
    let children = filtered.filter((c) => c.parent_id === root.id)
    if (sort) children = [...children].sort((a, b) => a.sort_order - b.sort_order)

    const node: CategoryTreeNode = {
      key: root.id,
      label: root.name,
      category: root,
      depth: 0,
    }

    if (children.length > 0) {
      node.children = children.map((c) => ({
        key: c.id,
        label: c.name,
        category: c,
        depth: 1,
      }))
    }

    return node
  })
}
