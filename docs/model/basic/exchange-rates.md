# exchange_rates（汇率表）

多币种换算预留，存储货币对汇率。

## 表结构

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT PK | 汇率记录 UUID v7 |
| base_code | TEXT FK | 基础货币代码（如 USD） |
| quote_code | TEXT FK | 报价货币代码（如 CNY） |
| rate | REAL | 汇率值（1 base = ? quote） |
| priced_at | TEXT | 行情采集时间（ISO 8601） |
| source | TEXT | 数据来源（manual, api, close 等） |
| updated_at | TEXT | 更新时间 |
| version | INTEGER | 版本号 |
| device_id | TEXT | 设备标识 |
| UNIQUE | (base_code, quote_code) | 每货币对仅保留一行最新汇率 |

## 设计说明

- 唯一约束保证每对货币只保留一行最新汇率
- 应用层使用 upsert 更新汇率
- 汇率值表示 1 单位 base_code 等于多少 quote_code
- priced_at 记录行情采集时间，不参与取数，仅用于记录
- v_holdings 视图通过 exchange_rates 进行持仓市值和成本的币种折算
- 同时支持正向汇率和反向汇率的兜底折算（如 USD→CNY 缺失，使用 CNY→USD 的倒数）

## 参考

- Migration：`src-tauri/migrations/V001__initial.sql`
