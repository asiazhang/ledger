# Ledger 数据模型设计

## 设计概述

Ledger 采用 SQLite 作为本地数据库，面向**多设备同步的离线优先**架构设计。

### 核心设计原则

1. **离线优先**：所有数据本地存储，支持无网络环境下正常使用
2. **多设备同步**：通过同步字段（device_id, updated_at, version, is_deleted）支持 LWW（Last Write Wins）冲突解决
3. **软删除**：所有主表使用 `is_deleted` 标志，不物理删除数据，保证同步一致性
4. **金额整数化**：所有金额以「分」为单位存储为整数，避免浮点精度问题
5. **UUID v7 主键**：全局唯一标识符，时间有序，适合分布式场景

---

## 实体索引

### 核心实体
- [currencies（币种）](./basic/currencies.md)
- [accounts（账户）](./basic/accounts.md)
- [categories（分类）](./basic/categories.md)
- [transactions（交易）](./basic/transactions.md)
- [budgets（预算）](./basic/budgets.md)
- [exchange_rates（汇率）](./basic/exchange-rates.md)

### 投资相关实体
- [instruments（金融工具）](./investment/instruments.md)
- [security_transactions（证券交易扩展）](./investment/security-transactions.md)
- [security_lots（持仓批次）](./investment/security-lots.md)
- [security_lot_sales（批次卖出匹配）](./investment/security-lot-sales.md)
- [market_prices（市场价格）](./investment/market-prices.md)
- [v_holdings（持仓视图）](./investment/holdings-view.md)

### 搜索索引
- [search_transactions（交易搜索索引）](./basic/search-transactions.md)

### 计划交易
- [scheduled_transactions 模块概览](./scheduled-transactions/index.md)
- [installment_plans（分期计划）](./scheduled-transactions/installment.md)
- [subscription_plans（订阅计划）](./scheduled-transactions/subscription.md)
- [scheduled_transfer_plans（计划转账）](./scheduled-transactions/scheduled-transfer.md)

---

## 关系图

```
currencies (币种)  ←──  exchange_rates (汇率)
    │
    ↓
accounts (账户)  ←── transactions (交易)  ──→  categories (分类)
    │                      │                        │
    │                      ├── security_transactions  budgets (预算)
    │                      │        │
    │                      │   instruments (金融工具)
    │                      │        │
    │                      │   security_lots (持仓批次)
    │                      │        │
    │                      │   security_lot_sales (批次卖出匹配)
    │                      │        │
    │                      │   market_prices (市场价格)
    │                      │
    │                      └── search_reindex_queue
    │                               │
    │                          search_transactions (FTS5 索引)
    │
    ↓
scheduled_transactions (计划交易)
    │
    ├── scheduled_transaction_occurrences (已发生执行)
    │
    ├── installment_plans (分期扩展)
    ├── subscription_plans (订阅扩展)
    └── scheduled_transfer_plans (计划转账扩展)
```

**核心关系**：
- accounts.currency_code → currencies.code
- transactions.account_id → accounts.id
- transactions.to_account_id → accounts.id（转账转入）
- transactions.category_id → categories.id
- transactions.refund_of_transaction_id → transactions.id（自引用）
- budgets.category_id → categories.id
- security_transactions.transaction_id → transactions.id
- security_transactions.instrument_id → instruments.id
- security_lots.account_id → accounts.id
- security_lots.instrument_id → instruments.id
- security_lots.buy_transaction_id → security_transactions.transaction_id
- security_lot_sales.sell_transaction_id → security_transactions.transaction_id
- security_lot_sales.lot_id → security_lots.id
- market_prices.instrument_id → instruments.id
- exchange_rates.base_code / quote_code → currencies.code
- scheduled_transactions.account_id → accounts.id
- scheduled_transactions.category_id → categories.id
- scheduled_transactions.to_account_id → accounts.id（计划转账转入）
- scheduled_transaction_occurrences.scheduled_transaction_id → scheduled_transactions.id
- installment_plans.id → scheduled_transactions.id
- subscription_plans.id → scheduled_transactions.id
- scheduled_transfer_plans.id → scheduled_transactions.id

---

## 同步机制

### 同步字段

所有主表（accounts, categories, transactions, budgets, security_lots, instruments, market_prices, exchange_rates）均携带：

- **device_id**：创建/最后修改设备标识
- **updated_at**：最后修改时间（UTC ISO 8601）
- **version**：版本号（每次修改 +1）
- **is_deleted**：软删除标志（0/1）

### LWW 冲突解决

- 比较 updated_at 时间戳，取最新者胜出
- version 字段用于乐观锁和变更追踪
- 软删除保证删除操作可同步

### 同步索引

所有主表均建立 `(updated_at, device_id)` 复合索引，用于高效查询某设备在某时间后的变更。

---

## 金额存储策略

### 整数存储

- 所有金额字段以「分」为单位存储为 INTEGER
- 避免浮点精度问题
- 展示时根据 currencies.decimal_places 格式化

### 多币种支持

- transactions 同时存储 amount_cents（原始币种）和 amount_native_cents（本位币）
- 当前实现中两者 1:1 相等，预留多币种换算能力
- exchange_rates 表提供汇率数据
- v_holdings 视图通过汇率折算计算账户币种市值

---

## 主键策略

### UUID v7

- 所有主表主键使用 UUID v7（TEXT 类型存储）
- 全局唯一，适合分布式场景
- 时间有序，利于索引和排序
- 默认分类使用基于 name+kind 的确定性 UUID v5，保证所有设备初始化后默认分类的 ID 一致

---

## 扩展性设计

### 已预留能力

1. **多币种**：exchange_rates 表、transactions.amount_native_cents 字段
2. **投资交易**：完整的证券交易、持仓批次、盈亏计算体系
3. **分类层次**：支持两级分类，可扩展更深层级
4. **软删除**：所有主表支持，保证同步一致性
5. **版本控制**：version 字段支持乐观锁和变更追踪

### 当前未实现

1. **多币种换算**：汇率表存在，但当前 MVP 阶段 transactions 的 amount_native_cents 与 amount_cents 1:1 相等，尚未根据实际汇率折算
2. **分类深层级**：数据库支持两级，未实现更深层级
3. **计划交易自动执行**：scheduled_transactions 模块 schema 完整，尚未接入定时执行逻辑

---

## 参考

- Migrations：`src-tauri/migrations/V001__initial.sql`, `V002__investment.sql`, `V003__seed_defaults.sql`
