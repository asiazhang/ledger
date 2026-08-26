# categories（分类表）

支出和收入的分类体系，支持两级层次结构。

## 表结构

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT PK | 分类 UUID v7 |
| name | TEXT | 分类名称（如「餐饮」「工资」） |
| kind | TEXT | 分类类型：`income`（收入）或 `expense`（支出） |
| parent_id | TEXT FK | 父分类 ID（NULL 表示顶级分类） |
| icon | TEXT | 图标名称（可选） |
| sort_order | INTEGER | 排序序号（默认 0，同级手动排序） |
| created_at | TEXT | 创建时间 |
| updated_at | TEXT | 最后修改时间 |
| version | INTEGER | 版本号 |
| device_id | TEXT | 设备标识 |
| is_deleted | INTEGER | 软删除标志 |

## 层次结构

- 顶级分类：`parent_id` 为 NULL
- 二级分类：`parent_id` 指向同表 `id`
- 父分类删除时，子分类 `parent_id` 置空（ON DELETE SET NULL）

## 默认种子数据

默认分类（支出顶级 13 个 / 支出二级 63 个 / 收入顶级 5 个 / 收入二级 11 个）与币种种子由 `V004__seed_defaults.sql` 定义，完整清单以该迁移为唯一事实来源，此处不重复罗列。

## 设计说明

- 默认分类使用基于 name+kind 的确定性 UUID v5，保证所有设备初始化后默认分类的 ID 一致
- 分类名不在交易搜索索引中（V005 收窄后的搜索范围），改名不触发索引重建
- 交易（transactions）的 category_id 删除时置空（ON DELETE SET NULL）
- 预算（budgets）的 category_id 删除时受限（ON DELETE RESTRICT），防止有预算的分类被误删
- 计划交易（scheduled_transactions）的 category_id 关联分类（转账类计划通常为空）

## 索引

- `idx_categories_parent`：(parent_id) 用于查询子分类
- `idx_categories_sync`：(updated_at, device_id) 用于同步

## 参考

- Migration：`src-tauri/migrations/V003__seed_defaults.sql`
