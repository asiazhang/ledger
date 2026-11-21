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

/**
 * 分类路径映射（issue #2187）：一次遍历把分类清单整体折叠为
 * 「分类标识 → 完整展示路径」的 Map，供消费方按标识 O(1) 查表取路径——
 * 替代逐次调用 categoryPath 时线性遍历重建临时映射的重复开销。
 * 路径口径与 categoryPath 单条查询完全一致：顶级为自身名，子分类为
 * 「父 > 子」，父级解析不到（孤儿引用）退自身名；同一输入下两函数结果相同。
 * 调用方传入含软删全量清单即可同时缓存软删分类的历史路径（历史交易口径）。
 */
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
  // 单条查询复用同一口径（buildCategoryPathMap 折叠后查表），双份口径不再并存；
  // 本纯函数已不在热路径（store 消费整体折叠缓存，见 issue #2187）。
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
