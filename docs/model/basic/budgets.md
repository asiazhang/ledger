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
- 预算金额以「分」为单位存储，写入校验要求为正数（0 或负数拒绝）
- 预算只能设置在支出分类上（收入分类与不存在的分类均拒绝）
- 同「分类 + 周期」只允许一条未删除预算，重复创建明确拒绝（提示「该分类已存在按月/按年预算，可编辑该预算的金额」），不静默覆盖；软删后可重新创建
- 预算开始日期（start_date）为冻结残留的记录字段：仅创建时记录用途，不参与进度计算
- 预算不可改分类/周期（改法为删旧建新）；金额可经编辑命令修改（`update_budget`，仅接受金额），沿用软删除同一套 updated_at/version/device_id 更新机制，金额与支出分类校验复用创建侧逻辑
- 分类删除时受限（ON DELETE RESTRICT），防止有预算的分类被误删
- 预算进度通过聚合 transactions 中对应分类（含子分类）的支出净额计算：spent = `expense_net`（毛支出 − 退款），口径与报表分类净值一致（由 `transaction::amount` 的 kind→度量矩阵驱动）；时间窗口为当前自然周期（月预算=当前自然月，年预算=当前自然年），随时间自动滚动，与存储的 start_date 无关（issue #182 永久滚动预算）

## 索引

- `idx_budgets_sync`：(updated_at, device_id) 用于同步

## 参考

- Migration：`src-tauri/migrations/V001__initial.sql`
