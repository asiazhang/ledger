# 导入 API

### `preview_import`

预览导入文件（仅解析不写库）。

- **命令名**：`preview_import`
- **参数**：`{ req: { path: string } }`
- **返回**：`ImportedRow[]`

```ts
interface ImportedRow {
  date: string
  amount_cents: number
  note: string
  category_name: string | null
}
```

- **后端**：`src-tauri/src/commands/import.rs:7`
- **支持格式**：`.csv`（使用 `csv` crate flexible 模式）、`.xlsx` / `.xls`（使用 `calamine`）
- **列名匹配**：支持中英文列名
  - `date` / `日期`
  - `amount` / `金额`
  - `note` / `备注` / `描述`
  - `category` / `分类`
- **金额解析**：`parse_amount_cents` 支持千分位逗号与负数
- **行为**：空日期行跳过，仅解析不写库
