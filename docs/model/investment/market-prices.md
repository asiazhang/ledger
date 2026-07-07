# market_prices（市场价格表）

每个 instrument 仅保留最新价格，用于计算持仓市值和未实现盈亏。

## 表结构

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT PK | 价格记录 UUID v7 |
| instrument_id | TEXT FK | 关联金融工具（ON DELETE CASCADE） |
| price_cents | INTEGER | 最新价（分） |
| currency_code | TEXT FK | 报价币种 |
| priced_at | TEXT | 行情日期（ISO 8601 日期格式） |
| source | TEXT | 数据来源（如 yahoo, manual） |
| created_at | TEXT | 创建时间 |
| updated_at | TEXT | 更新时间 |
| version | INTEGER | 版本号 |
| device_id | TEXT | 设备标识 |
| UNIQUE | (instrument_id) | 每个工具仅保留最新价格 |

## 设计说明

- 每个 instrument 仅保留最新价格，用于计算持仓市值和未实现盈亏
- 唯一约束 (instrument_id) 保证每个工具只有一行最新价格
- 应用层使用 upsert 更新价格
- priced_at 记录行情日期，updated_at 记录写入时间
- 工具删除时级联删除（ON DELETE CASCADE），价格数据跟随工具生命周期

## 索引

- `idx_market_prices_instrument`：(instrument_id) 用于查询工具最新价格

## 参考

- Migration：`src-tauri/migrations/V002__investment.sql`
