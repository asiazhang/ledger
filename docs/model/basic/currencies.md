# currencies（币种表）

存储系统支持的货币类型。

## 表结构

| 字段 | 类型 | 说明 |
|------|------|------|
| code | TEXT PK | ISO 4217 货币代码（如 CNY, USD, EUR） |
| name | TEXT | 货币中文名称 |
| symbol | TEXT | 展示符号（如 ¥, $, €） |
| decimal_places | INTEGER | 最小小数位，用于格式化展示 |

## 特点

- 主键为货币代码，非 UUID
- 无同步字段（系统级字典数据）
- 默认种子数据包含 11 种常用货币

## 默认种子数据

11 种常用货币（CNY / USD / EUR / JPY / GBP / HKD / AUD / CAD / KRW / SGD / CHF）由 `V004__seed_defaults.sql` 定义（`INSERT OR IGNORE`），名称/symbol/decimal_places 以该迁移为唯一事实来源，此处不重复罗列。注意符号与小数位细节以迁移为准（如 JPY `decimal_places=0`）。

## 被引用关系

- accounts.currency_code → currencies.code（ON DELETE RESTRICT）
- transactions.currency_code → currencies.code（ON DELETE RESTRICT）
- instruments.currency_code → currencies.code（ON DELETE RESTRICT）
- security_lots.currency_code → currencies.code（ON DELETE RESTRICT）
- security_lot_sales.currency_code → currencies.code（ON DELETE RESTRICT）
- market_prices.currency_code → currencies.code（ON DELETE RESTRICT）
- exchange_rates.base_code → currencies.code（ON DELETE RESTRICT）
- exchange_rates.quote_code → currencies.code（ON DELETE RESTRICT）
- scheduled_transactions.currency_code → currencies.code

## 参考

- Migration：`src-tauri/migrations/V003__seed_defaults.sql`
