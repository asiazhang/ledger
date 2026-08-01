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

| code | name | symbol | decimal_places |
|------|------|--------|----------------|
| CNY | 人民币 | ¥ | 2 |
| USD | 美元 | $ | 2 |
| EUR | 欧元 | € | 2 |
| JPY | 日元 | ¥ | 2 |
| GBP | 英镑 | £ | 2 |
| HKD | 港币 | HK$ | 2 |
| AUD | 澳元 | A$ | 2 |
| CAD | 加元 | C$ | 2 |
| KRW | 韩元 | ₩ | 0 |
| SGD | 新加坡元 | S$ | 2 |
| CHF | 瑞士法郎 | Fr. | 2 |

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
