# budgets（预算表）

按支出分类设置周期预算。

## 表结构

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT PK | 预算 UUID v7 |
| category_id | TEXT FK | 关联支出分类 |
| period | TEXT | 预算周期：`monthly`（按月）或 `yearly`（按年） |
| amount_cents | INTEGER | 预算金额上限（分） |
| start_date | TEXT | 预算开始日期（ISO 8601） |
| created_at | TEXT | 创建时间 |
| updated_at | TEXT | 最后修改时间 |
| version | INTEGER | 版本号 |
| device_id | TEXT | 设备标识 |
| is_deleted | INTEGER | 软删除标志 |

## 预算周期

| 周期 | 含义 |
|------|------|
| `monthly` | 按月预算 |
| `yearly` | 按年预算 |

## 设计说明

- 预算与支出分类一一对应，通过 category_id 关联
- 预算金额以「分」为单位存储
- 预算开始日期（start_date）决定预算周期生效时间
- 分类删除时受限（ON DELETE RESTRICT），防止有预算的分类被误删
- 预算进度通过聚合 transactions 中对应分类（含子分类）的支出净额计算：spent = `expense_net`（毛支出 − 退款），口径与报表分类净值一致（由 `transaction::amount` 的 kind→度量矩阵驱动）；时间窗口按预算 `start_date` 的月份

## 索引

- `idx_budgets_sync`：(updated_at, device_id) 用于同步

## 参考

- Migration：`src-tauri/migrations/V001__initial.sql`
