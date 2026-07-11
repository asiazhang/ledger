# 汇率 API

### `list_exchange_rates`

列出所有汇率。

- **命令名**：`list_exchange_rates`
- **参数**：无
- **返回**：`ExchangeRate[]`

```ts
interface ExchangeRate {
  id: string
  base_code: string
  quote_code: string
  rate: number
  priced_at: string
  source: string | null
  updated_at: string
  version: number
  device_id: string
}
```

- **后端**：`src-tauri/src/commands/investment.rs:263`
- **排序**：按 `base_code, quote_code`

### `create_exchange_rate`

创建或更新汇率。

- **命令名**：`create_exchange_rate`
- **参数**：`{ input: ExchangeRateInput }`

```ts
interface ExchangeRateInput {
  base_code: string
  quote_code: string
  rate: number     // 必须 > 0
  priced_at: string
  source?: string | null
}
```

- **返回**：`string`（汇率 ID）
- **后端**：`src-tauri/src/commands/investment.rs:276`
- **幂等**：按 `(base_code, quote_code)` UPSERT
