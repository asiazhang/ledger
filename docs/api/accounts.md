# 账户 API

### `list_accounts`

列出所有未删除账户。

- **命令名**：`list_accounts`
- **参数**：无
- **返回**：`Account[]`

```ts
interface Account {
  id: string
  name: string
  type: AccountType  // 'cash' | 'bank' | 'credit' | 'ewallet' | 'investment' | 'debt' | 'receivable' | 'other'
  currency_code: string
  initial_balance_cents: number
  created_at: string
  updated_at: string
  version: number
  device_id: string
  is_deleted: boolean
}
```

- **后端**：`src-tauri/src/commands/accounts.rs:9`
- **过滤**：`is_deleted=0`，按 `created_at` 排序

### `create_account`

创建账户。

- **命令名**：`create_account`
- **参数**：`{ input: AccountInput }`

```ts
interface AccountInput {
  name: string
  type: AccountType
  currency_code: string
  initial_balance_cents?: number  // 默认 0
}
```

- **返回**：`string`（新账户 ID）
- **后端**：`src-tauri/src/commands/accounts.rs:20`

### `delete_account`

软删除账户。

- **命令名**：`delete_account`
- **参数**：`{ id: string }`
- **返回**：`void`
- **后端**：`src-tauri/src/commands/accounts.rs:43`
- **行为**：设置 `is_deleted=1`，不物理删除

### `list_account_balances`

列出各账户当前余额。

- **命令名**：`list_account_balances`
- **参数**：无
- **返回**：`AccountBalance[]`

```ts
interface AccountBalance {
  account: Account
  balance_cents: number  // 实时计算，不持久化
}
```

- **后端**：`src-tauri/src/commands/accounts.rs:107`
- **余额计算**：`初始余额 + 收入 - 支出 + 转入 - 转出 + 退款`
