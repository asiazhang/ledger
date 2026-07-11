# 分类 API

### `list_categories`

列出所有未删除分类。

- **命令名**：`list_categories`
- **参数**：无
- **返回**：`Category[]`

```ts
interface Category {
  id: string
  name: string
  kind: string  // 'income' | 'expense'
  parent_id: string | null
  icon: string | null
  color: string | null
  created_at: string
  updated_at: string
  version: number
  device_id: string
  is_deleted: boolean
}
```

- **后端**：`src-tauri/src/commands/categories.rs:8`
- **过滤**：`is_deleted=0`，按 `kind, created_at` 排序

### `create_category`

创建分类。

- **命令名**：`create_category`
- **参数**：`{ input: CategoryInput }`

```ts
interface CategoryInput {
  name: string
  kind: string
  parent_id?: string | null
}
```

- **返回**：`string`（新分类 ID）
- **后端**：`src-tauri/src/commands/categories.rs:19`

### `delete_category`

软删除分类。

- **命令名**：`delete_category`
- **参数**：`{ id: string }`
- **返回**：`void`
- **后端**：`src-tauri/src/commands/categories.rs:32`
