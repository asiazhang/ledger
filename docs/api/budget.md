# 预算 API

### `list_budgets`

列出所有未删除预算。

- **命令名**：`list_budgets`
- **参数**：无
- **返回**：`Budget[]`

```ts
interface Budget {
  id: string
  category_id: string
  period: BudgetPeriod  // 'monthly' | 'yearly'
  amount_cents: number
  start_date: string
  created_at: string
  updated_at: string
  version: number
  device_id: string
  is_deleted: boolean
}
```

- **后端**：`src-tauri/src/commands/budget.rs:8`
- **过滤**：`is_deleted=0`，按 `created_at` 排序

### `create_budget`

创建预算。

- **命令名**：`create_budget`
- **参数**：`{ input: BudgetInput }`

```ts
interface BudgetInput {
  category_id: string
  period?: BudgetPeriod  // 默认 monthly
  amount_cents: number
  start_date: string
}
```

- **返回**：`string`（新预算 ID）
- **后端**：`src-tauri/src/commands/budget.rs:19`

### `delete_budget`

软删除预算。

- **命令名**：`delete_budget`
- **参数**：`{ id: string }`
- **返回**：`void`
- **后端**：`src-tauri/src/commands/budget.rs:43`

### `budget_progress`

预算执行进度。

- **命令名**：`budget_progress`
- **参数**：无
- **返回**：`BudgetProgress[]`

```ts
interface BudgetProgress {
  budget: Budget
  category_name: string
  spent_cents: number
  over_budget: boolean
}
```

- **后端**：`src-tauri/src/commands/budget.rs:53`
- **计算**：当月支出（含子分类，refund 取负值）对比预算金额
- **时间窗口**：按预算 `start_date` 的月份
