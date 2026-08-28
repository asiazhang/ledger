# instruments（金融工具字典表）

统一维护股票、基金、债券、ETF 等金融工具的基础信息。

## 表结构

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT PK | 工具 UUID v7 |
| symbol | TEXT | 代码（如 "600519.SH", "NVDA", "000001"） |
| instrument_type | TEXT | 金融工具类型（见下方枚举） |
| name | TEXT | 名称（可选，如 "贵州茅台"） |
| currency_code | TEXT FK | 报价币种 |
| market | TEXT | 交易市场：`sh` / `sz` / `hk` / `unknown`（V002 建表即有，默认 unknown） |
| created_at | TEXT | 创建时间 |
| updated_at | TEXT | 最后修改时间 |
| version | INTEGER | 版本号 |
| device_id | TEXT | 创建设备/最后修改设备标识 |
| UNIQUE | (symbol, instrument_type) | 同一代码和类型唯一 |

## 金融工具类型枚举

| 类型 | 含义 |
|------|------|
| `stock` | 股票 |
| `fund` | 基金 |
| `bond` | 债券 |
| `etf` | ETF |
| `other` | 其他 |

## 设计说明

- 交易、持仓批次通过 instrument_id 关联，避免重复录入名称和币种
- currency_code 表示该工具的报价和交易币种
- 唯一约束 (symbol, instrument_type) 防止同一代码同一类型重复录入
- 工具删除时受限（ON DELETE RESTRICT），防止关联的 security_transactions / security_lots 孤立

## 被引用关系

- security_transactions.instrument_id → instruments.id（ON DELETE RESTRICT）
- security_lots.instrument_id → instruments.id（ON DELETE RESTRICT）
- market_prices.instrument_id → instruments.id（ON DELETE CASCADE）

## 查询约定（服务端分页）

- `list_instruments` 支持服务端分页与搜索：`filter = { search?, market?, page?, page_size? }`，返回 `{ items, total }`。
- 默认 `page=1`、`page_size=50`（上限 500），排序固定 `ORDER BY symbol`（分页依赖稳定排序）。
- `search` 对 `symbol` / `name` 做大小写不敏感子串匹配（`LOWER` + `LIKE`），`market` 精确匹配。
- 标的全量可达万级（全量同步自东方财富，入口在投资页"标的"子视图），标的浏览列表与两个标的筛选下拉均走服务端分页/远程搜索，不在前端全量驻留；而交易列表、卖出明细等量级有上限的数据沿用客户端分页（NDataTable 内置）。两者不一致是有意为之：量级不同，内存代价不同。

## 参考

- Migration：`src-tauri/migrations/V002__investment.sql`（基础结构，含 market 列）
