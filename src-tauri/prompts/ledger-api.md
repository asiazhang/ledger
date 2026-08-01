# Ledger API 系统提示词

## 概述

- **基础地址**: `http://127.0.0.1:9527/api/v1`
- **请求格式**: JSON (`Content-Type: application/json`)
- **响应格式**: JSON
- **错误格式**: `{ "kind": "<ErrorKind>", "message": "<中文错误描述>" }`
  - `Invalid` (400) — 参数校验失败
  - `Parse` (400) — 导入解析失败
  - `NotFound` (404) — 数据不存在
  - `Db` (500) — 数据库错误
  - `Io` (500) — IO 错误

> **⚠️ 重要约定：所有金额均以分为单位存储。** 字段名统一带 `_cents` 后缀（如 `amount_cents`、`initial_balance_cents`）。发送/接收金额字段时，请始终使用整数分，切勿使用元或浮点数。

---

## 端点详细说明

### 1. 列出所有账户

`GET /api/v1/accounts`

**请求**: 无

**响应**: `Account[]`

```json
[
  {
    "id": "uuid-string",
    "name": "现金钱包",
    "type": "cash",
    "currency_code": "CNY",
    "initial_balance_cents": 0,
    "created_at": "2026-01-01T00:00:00Z",
    "updated_at": "2026-01-01T00:00:00Z",
    "version": 1,
    "device_id": "device-uuid",
    "is_deleted": false
  }
]
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | string | UUID |
| `name` | string | 账户名称 |
| `type` | string | 枚举值: `cash` / `bank` / `credit` / `ewallet` / `investment` / `debt` / `receivable` / `other` |
| `currency_code` | string | 币种代码, 如 `CNY` / `USD` |
| `initial_balance_cents` | integer | 初始余额(分) |
| `is_deleted` | boolean | 软删除标记 |

---

### 2. 创建账户

`POST /api/v1/accounts`

**请求**: `AccountInput`

```json
{
  "name": "交通银行储蓄卡",
  "type": "bank",
  "currency_code": "CNY",
  "initial_balance_cents": 0
}
```

**字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 账户名称 |
| `type` | string | 是 | 账户类型, 枚举值同上 |
| `currency_code` | string | 是 | 币种代码 |
| `initial_balance_cents` | integer | 否 | 初始余额(分), 默认 0 |

**响应**: `201 Created` + 账户 ID (string)

```json
"uuid-of-new-account"
```

---

### 3. 列出所有分类

`GET /api/v1/categories`

**请求**: 无

**响应**: `Category[]`

```json
[
  {
    "id": "uuid-string",
    "name": "餐饮",
    "kind": "expense",
    "parent_id": null,
    "icon": null,
    "sort_order": 0,
    "created_at": "2026-01-01T00:00:00Z",
    "updated_at": "2026-01-01T00:00:00Z",
    "version": 1,
    "device_id": "device-uuid",
    "is_deleted": false
  }
]
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | string | UUID |
| `name` | string | 分类名称 |
| `kind` | string | 枚举值: `income` / `expense` |
| `parent_id` | string\|null | 父分类 ID, 用于三级分类体系 |
| `icon` | string\|null | 图标 emoji |
| `sort_order` | integer | 排序序号 |

---

### 4. 创建分类

`POST /api/v1/categories`

**请求**: `CategoryInput`

```json
{
  "name": "交通出行",
  "kind": "expense",
  "parent_id": null,
  "icon": "🚗"
}
```

**字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 分类名称 |
| `kind` | string | 是 | `income` / `expense` |
| `parent_id` | string\|null | 否 | 父分类 ID |
| `icon` | string\|null | 否 | 图标 emoji |

**响应**: `201 Created` + 分类 ID (string)

```json
"uuid-of-new-category"
```

---

### 5. 批量创建交易

`POST /api/v1/transactions/batch`

**请求**: `{ "transactions": TransactionInput[], "dedup": bool }` — `dedup` 缺省 `true`（重复导入自动跳过）

```json
{
  "dedup": true,
  "transactions": [
    {
      "kind": "income",
      "amount_cents": 500000,
      "currency_code": "CNY",
      "account_id": "uuid-of-account",
      "to_account_id": null,
      "category_id": "uuid-of-category",
      "note": "1月工资",
      "date": "2026-01-15"
    },
    {
      "kind": "expense",
      "amount_cents": 3500,
      "currency_code": "CNY",
      "account_id": "uuid-of-account",
      "category_id": "uuid-of-category",
      "note": "午餐",
      "date": "2026-01-15"
    },
    {
      "kind": "transfer",
      "amount_cents": 100000,
      "currency_code": "CNY",
      "account_id": "uuid-of-source-account",
      "to_account_id": "uuid-of-target-account",
      "date": "2026-01-15"
    }
  ]
}
```

**字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `transactions` | array | 是 | 交易数组（结构见下） |
| `dedup` | bool | 否 | 是否去重，缺省 `true` |
| `kind` | string | 是 | `income` / `expense` / `transfer` |
| `amount_cents` | integer | 是 | 金额(分), **必须大于 0** |
| `currency_code` | string | 是 | 币种代码 |
| `account_id` | string | 是 | 账户 ID (对 transfer 为转出账户) |
| `to_account_id` | string\|null | 否 | **transfer 必填** — 转入账户 ID |
| `category_id` | string\|null | 否 | 分类 ID |
| `note` | string\|null | 否 | 备注 |
| `date` | string | 是 | 日期, 格式 `YYYY-MM-DD` |

**响应**: `CreateTransactionResult[]`

```json
[
  { "success": true, "duplicate": false, "id": "uuid-1", "error": null },
  { "success": true, "duplicate": true, "id": null, "error": null },
  { "success": false, "duplicate": false, "id": null, "error": "金额必须大于 0" }
]
```

**去重规则**:
- `dedup: true`（缺省）时，命中已存在（`is_deleted=0`）的同 `dedup_hash` 交易则跳过，返回 `{success: true, duplicate: true, id: null}`——既非新建也非失败，无需重试、不应上报错误
- `dedup: false` 时不做去重，重复写入成功（新增行）
- `dedup_hash = sha256(date|kind|amount_cents|currency_code|account_id|to_account_id)`，排除 note/category，`to_account_id` 缺省拼空串
- 只匹配 `is_deleted=0` 的交易：软删除的交易不占去重位，重跑会重新写入
- `dedup_hash` 导入后保持不变，编辑/同步无特殊处理

**业务约束**:
- ⚠️ `amount_cents` 必须 > 0
- ⚠️ `kind = "transfer"` 必须同时指定 `account_id`（转出）和 `to_account_id`（转入）
- ⚠️ `category_id` 和 `account_id` 必须事先存在于数据库中
- 日期格式严格为 `YYYY-MM-DD`

**重要边界**:
- **逐条校验, 不阻塞整体**: 单条交易的业务校验失败（`Invalid` 错误，如金额为 0、缺少 `to_account_id`）会在结果数组中标记 `success: false` 并附带 `error` 信息，不会影响其他交易
- **其他错误整体回滚**: 若发生数据库等系统性错误，整个批次回滚并返回 500 错误
- 成功写入的交易始终在一个数据库事务中提交

---

## 迁移场景典型流程

以从其他记账 App 导入 CSV 数据为例：

1. **拉取已有数据** — `GET /api/v1/categories` 获取分类列表，构造 `分类名称 → 分类 ID` 映射表；`GET /api/v1/accounts` 获取账户列表，构造 `账户名称 → 账户 ID` 映射表
2. **补齐缺失数据** — 对 CSV 中 `category` 列在映射表中找不到的分类，调用 `POST /api/v1/categories` 创建；对不存在的账户，调用 `POST /api/v1/accounts` 创建
3. **批量写入交易** — 将 CSV 行转换为 `TransactionInput[]`，按金额正负决定 `kind`（正为 `income`，负为 `expense`），填充 `account_id` 和 `category_id` 后以 `{transactions, dedup}` 包裹调用 `POST /api/v1/transactions/batch` 批量写入；`dedup` 缺省开启，命中 `duplicate: true` 的行说明已存在，跳过即可
