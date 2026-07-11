# 投资 API

### `list_instruments`

列出所有金融工具。

- **命令名**：`list_instruments`
- **参数**：无
- **返回**：`Instrument[]`

```ts
interface Instrument {
  id: string
  symbol: string
  type: InstrumentType  // 'stock' | 'fund' | 'bond' | 'etf' | 'other'
  name: string | null
  currency_code: string
  created_at: string
  updated_at: string
  version: number
  device_id: string
}
```

- **后端**：`src-tauri/src/commands/investment.rs:371`
- **排序**：按 `symbol`

### `create_instrument`

创建金融工具，已存在则返回已有 ID（幂等）。

- **命令名**：`create_instrument`
- **参数**：`{ input: InstrumentInput }`

```ts
interface InstrumentInput {
  symbol: string
  type: InstrumentType
  name?: string | null
  currency_code: string
}
```

- **返回**：`string`（工具 ID）
- **后端**：`src-tauri/src/commands/investment.rs:384`
- **幂等**：按 `(symbol, instrument_type)` 查重，存在则直接返回 ID

### `list_holdings`

列出当前持仓。

- **命令名**：`list_holdings`
- **参数**：无
- **返回**：`Holding[]`

```ts
interface Holding {
  id: string               // 格式: account_id-instrument_id-currency_code
  account_id: string
  instrument_id: string
  quantity: number
  cost_basis_cents: number
  cost_currency_code: string
  latest_price_cents: number | null
  latest_price_currency_code: string | null
  market_value_cents: number | null
  unrealized_pnl_cents: number | null
  updated_at: string
}
```

- **后端**：`src-tauri/src/commands/investment.rs:249`
- **数据源**：`v_holdings` SQL 视图，实时聚合
