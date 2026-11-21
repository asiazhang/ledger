import type { Category, CategoryKind } from "@ledger/types";

export interface CategoryTreeNode {
  key: string;
  label: string;
  category: Category;
  depth: number;
  children?: CategoryTreeNode[];
  [key: string]: unknown;
}

function makeCategoryMap(categories: Iterable<Category>): Map<string, Category> {
  const m = new Map<string, Category>();
  for (const c of categories) m.set(c.id, c);
  return m;
}

export function rootCategories(categories: Category[]): Category[] {
  return categories.filter((c) => c.parent_id == null);
}

export function categoryChildren(categories: Category[], parentId: string): Category[] {
  return categories.filter((c) => c.parent_id === parentId);
}

export function buildCategoryPathMap(categories: Iterable<Category>): Map<string, string> {
  const byId = makeCategoryMap(categories);
  const paths = new Map<string, string>();
  for (const c of byId.values()) {
    const parent = c.parent_id != null ? byId.get(c.parent_id) : undefined;
    paths.set(c.id, parent ? `${parent.name} > ${c.name}` : c.name);
  }
  return paths;
}

export function categoryPath(categories: Category[], id: string | null | undefined): string {
  return buildCategoryPathMap(categories).get(id ?? "") ?? "";
}

export function categoryRoot(categories: Category[], categoryId: string): Category | undefined {
  const map = makeCategoryMap(categories);
  let cat = map.get(categoryId);
  if (!cat) return undefined;
  while (cat.parent_id != null) {
    const parent = map.get(cat.parent_id);
    if (!parent) break;
    cat = parent;
  }
  return cat;
}

export function buildCategoryTree(
  categories: Category[],
  options?: { kind?: CategoryKind; sort?: boolean },
): CategoryTreeNode[] {
  const { kind, sort = true } = options ?? {};
  const filtered = kind != null ? categories.filter((c) => c.kind === kind) : categories;

  let roots = filtered.filter((c) => c.parent_id == null);
  if (sort) roots = [...roots].sort((a, b) => a.sort_order - b.sort_order);

  return roots.map((root) => {
    let children = filtered.filter((c) => c.parent_id === root.id);
    if (sort) children = [...children].sort((a, b) => a.sort_order - b.sort_order);

    const node: CategoryTreeNode = {
      key: root.id,
      label: root.name,
      category: root,
      depth: 0,
    };

    if (children.length > 0) {
      node.children = children.map((c) => ({
        key: c.id,
        label: c.name,
        category: c,
        depth: 1,
      }));
    }

    return node;
  });
}
