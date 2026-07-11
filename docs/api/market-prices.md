# 市场价格 API

### `list_market_prices`

列出所有市场价格。

- **命令名**：`list_market_prices`
- **参数**：无
- **返回**：`MarketPrice[]`

```ts
interface MarketPrice {
  id: string
  instrument_id: string
  price_cents: number
  currency_code: string
  priced_at: string
  source: string | null
  created_at: string
  updated_at: string
  version: number
  device_id: string
}
```

- **后端**：`src-tauri/src/commands/investment.rs:316`
- **排序**：按 `instrument_id, priced_at DESC`

### `create_market_price`

创建或更新市场价格。

- **命令名**：`create_market_price`
- **参数**：`{ input: MarketPriceInput }`

```ts
interface MarketPriceInput {
  instrument_id: string
  price_cents: number    // 必须 > 0
  currency_code: string
  priced_at: string
  source?: string | null
}
```

- **返回**：`string`（价格 ID）
- **后端**：`src-tauri/src/commands/investment.rs:329`
- **幂等**：按 `instrument_id` UPSERT
