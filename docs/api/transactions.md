# 交易 API

### `list_transactions`

列出未删除交易。

- **命令名**：`list_transactions`
- **参数**：`{ limit?: number | null }`
- **返回**：`Transaction[]`

```ts
interface Transaction {
  id: string
  kind: string           // 'income' | 'expense' | 'transfer' | 'refund' | 'buy' | 'sell'
  amount_cents: number
  currency_code: string
  amount_native_cents: number
  account_id: string
  to_account_id: string | null
  category_id: string | null
  refund_of_transaction_id: string | null
  note: string | null
  date: string
  created_at: string
  updated_at: string
  version: number
  device_id: string
  is_deleted: boolean
}
```

- **后端**：`src-tauri/src/commands/transactions.rs:84`
- **排序**：`date DESC, created_at DESC`
- **限制**：传入 `limit` 时拼接 `LIMIT n`，否则返回全部

### `create_transaction`

创建单条交易。

- **命令名**：`create_transaction`
- **参数**：`{ input: TransactionInput }`

```ts
interface TransactionInput {
  kind: string
  amount_cents: number
  currency_code: string
  account_id: string
  to_account_id?: string | null
  category_id?: string | null
  refund_of_transaction_id?: string | null
  note?: string | null
  date: string
  instrument_id?: string | null   // buy/sell 时必填
  quantity?: number | null         // buy/sell 时必填
  price_cents?: number | null      // buy/sell 时必填
  fee_cents?: number | null        // buy/sell 时可选
}
```

- **返回**：`string`（新交易 ID）
- **后端**：`src-tauri/src/commands/transactions.rs:97`
- **通用校验**：
  - `transfer` 必须指定 `to_account_id`
  - `refund` 必须指定 `refund_of_transaction_id`，且原交易必须是 `expense`
  - 非 buy/sell 时 `amount_cents > 0`
  - buy/sell 转到 `investment::create_buy_transaction` / `create_sell_transaction` 处理
- **buy 特殊处理**（`src-tauri/src/commands/investment.rs:10`）：
  - 必须指定 `instrument_id`，`quantity > 0`，`price_cents > 0`
  - 账户必须是 `investment` 类型
  - 自动计算 `amount_cents = quantity * price_cents + fee_cents`
  - 同时创建 `security_lot` 记录
- **sell 特殊处理**（`src-tauri/src/commands/investment.rs:75`）：
  - 必须指定 `instrument_id`，`quantity > 0`，`price_cents > 0`
  - 账户必须是 `investment` 类型
  - 自动计算 `amount_cents = quantity * price_cents - fee_cents`
  - FIFO 匹配同账户同标的未卖出 lot，扣减 `remaining_quantity`
  - 写入 `security_lot_sales` 记录，计算已实现盈亏

### `create_transactions`

批量创建交易。

- **命令名**：`create_transactions`
- **参数**：`{ inputs: TransactionInput[] }`
- **返回**：`CreateTransactionResult[]`

```ts
interface CreateTransactionResult {
  success: boolean
  id: string | null
  error: string | null
}
```

- **后端**：`src-tauri/src/commands/transactions.rs:103`
- **事务行为**：整个批量在单个事务中执行。校验错误（`AppError::Invalid`）跳过单条继续，其他错误（如 DB 错误）回滚整个事务。
- **用途**：CSV/Excel 导入场景

### `delete_transaction`

软删除交易。

- **命令名**：`delete_transaction`
- **参数**：`{ id: string }`
- **返回**：`void`
- **后端**：`src-tauri/src/commands/transactions.rs:133`
- **buy 交易特殊处理**：
  - 如果部分卖出（`remaining_quantity < initial_quantity`），拒绝删除
  - 否则同时删除关联的 `security_lot` 和 `security_transactions` 记录
