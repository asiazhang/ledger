# 币种 API

### `list_currencies`

列出所有币种。

- **命令名**：`list_currencies`
- **参数**：无
- **返回**：`Currency[]`

```ts
interface Currency {
  code: string
  name: string
  symbol: string
  decimal_places: number
}
```

- **后端**：`src-tauri/src/commands/currencies.rs:8`
- **数据源**：`currencies` 表（系统字典，无同步字段）
