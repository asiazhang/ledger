# 报表 API

### `monthly_summary`

月度收支汇总。

- **命令名**：`monthly_summary`
- **参数**：`{ year: number }`
- **返回**：`MonthlySummary[]`

```ts
interface MonthlySummary {
  month: string         // 'YYYY-MM'
  income_cents: number
  expense_cents: number
  refund_cents: number
}
```

- **后端**：`src-tauri/src/commands/reports.rs:8`
- **聚合**：按 `substr(date,1,7)` 分组

### `category_shares`

分类占比统计。

- **命令名**：`category_shares`
- **参数**：`{ kind: string, month?: string | null }`

  - `kind`：`'expense'` 或 `'income'`
  - `month`：可选，格式 `'YYYY-MM'`，不传则查所有

- **返回**：`CategoryShare[]`

```ts
interface CategoryShare {
  category_id: string
  category_name: string  // 未分类返回 '未分类'
  amount_cents: number
}
```

- **后端**：`src-tauri/src/commands/reports.rs:23`
- **expense 特殊处理**：包含 `expense` 和 `refund`（refund 取负值）
