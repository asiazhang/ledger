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
| color | TEXT | 展示颜色（可选） |
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

### 支出顶级分类：13 个

餐饮、交通、购物、住房、娱乐、医疗、教育、其他支出、生活缴费、人情、金融保险、数码产品、汽车

### 支出二级分类：61 个

- 餐饮：早餐、午餐、晚餐、零食饮料、外卖、聚餐
- 交通：公交地铁、出租车、火车机票、共享出行
- 汽车：加油、充电、停车、过路费、保养、维修、洗车、年检、车险、美容改装、违章罚款
- 购物：服饰鞋包、日用百货、生鲜食材、美妆护肤、母婴用品
- 住房：房租、房贷、装修、家居家具、家用电器
- 娱乐：电影演出、游戏、旅行出游、订阅会员、健身运动
- 医疗：门诊挂号、药品、体检、住院手术
- 教育：书籍、培训课程、学费、文具
- 生活缴费：话费、宽带、水费、电费、燃气费、物业费
- 人情：礼金红包、请客送礼
- 金融保险：金融费用、寿险健康险、财产险
- 数码产品：手机、电脑、平板、耳机音箱、智能穿戴、游戏机、软件服务、数码配件

### 收入顶级分类：5 个

工资、奖金、投资收益、其他收入、兼职劳务

### 收入二级分类：11 个

- 工资：基本工资、加班费、补贴
- 奖金：年终奖、绩效奖金
- 投资收益：股票分红、基金收益、理财利息
- 兼职劳务：兼职、劳务报酬
- 其他收入：物品售出

## 设计说明

- 默认分类使用基于 name+kind 的确定性 UUID v5，保证所有设备初始化后默认分类的 ID 一致
- 交易（transactions）的 category_id 删除时置空（ON DELETE SET NULL）
- 预算（budgets）的 category_id 删除时受限（ON DELETE RESTRICT），防止有预算的分类被误删
- 计划交易（scheduled_transactions）的 category_id 关联分类（转账类计划通常为空）

## 索引

- `idx_categories_parent`：(parent_id) 用于查询子分类
- `idx_categories_sync`：(updated_at, device_id) 用于同步

## 参考

- Migration：`src-tauri/migrations/V003__seed_defaults.sql`
