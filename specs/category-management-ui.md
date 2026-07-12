# 分类配置 UI 优化

## 问题描述

当前 SettingsView 中的分类管理只是一个简单的"表单 + 数据表"：添加分类只有 name/kind/parent 三个字段，列表以平铺表格渲染（用 ID 字符串排序），没有视觉标识（icon/color），不能编辑已有分类，不能自定义排序。用户创建了较多分类后难以快速定位和管理。

## 解决方案

将分类管理从 SettingsView 中提取为独立的交互面板：支持可视化树形列表（缩进展示两级层级）、同级拖拽排序、行内展示 icon 和 color、编辑已有分类的 name/icon/color/parent，以及完整的增删操作。

## 用户故事

1. 作为记账用户，我希望以可视化树形（缩进两级）查看支出/收入分类，从而快速掌握分类层级结构。
2. 作为记账用户，我希望每个分类行展示其图标（emoji）和颜色色块，从而一眼识别不同分类。
3. 作为记账用户，我希望在同一层级内拖拽分类上下移动来排序，从而把高频分类放在前面。
4. 作为记账用户，我希望在弹窗中编辑已有分类的名称、图标、颜色和父分类，从而无需删除重建。
5. 作为记账用户，我希望新增分类时一步设置名称、类型（支出/收入）、父分类、图标和颜色，从而一开始就完整配置。
6. 作为记账用户，我希望删除分类时有确认提示，从而避免误删。
7. 作为记账用户，我希望自定义的排序在应用重启后保持不变，从而不会丢失排序结果。
8. 作为记账用户，我希望父分类下拉框只显示同类型（支出/收入匹配）的分类，从而不会创建不一致的层级关系。

## 实施决策

### Schema 变更
- 在 `categories` 表新增 `sort_order` 列（INTEGER，默认 0）。
- 迁移时对已有数据设 `sort_order = rowid`，使旧分类有一个稳定的默认排序。

### 后端命令
- **`update_category(id, input)`** — 更新 `name`、`icon`、`color`、`parent_id`。返回更新后的分类。校验：父分类 kind 必须一致，父分类不能是自身的后代（安全兜底，两级约束下主要是防御性校验）。
- **`reorder_categories(orders)`** — 接收 `{ id: string, sort_order: number }` 列表，在事务中批量更新。

### 前端组件结构
- 从 `SettingsView.vue` 中提取分类管理为独立组件 `CategoryManager.vue`（在 SettingsView 内渲染）。
- 组件使用 `NDataTable`，列包括：图标（emoji）、颜色色块、名称（缩进）、类型（标签）、操作（编辑/删除）。
- 每行有拖拽手柄（☰），通过原生 HTML5 Drag & Drop API 实现同级拖拽（不引入第三方库）。
- **拖拽行为**：仅限同级重排。变更父分类（调整层级）通过编辑弹窗完成，不走拖拽。
- 编辑弹窗使用 `NModal`，字段：名称、图标（emoji 输入或选择器）、颜色（`<input type="color">`）、父分类（按 kind 过滤的 NSelect）。
- 新增表单（面板顶部或"+"按钮）在已有 name/kind/parent 基础上增加 icon 和 color 字段。
- 数据通过 `store.loadCategories()` 加载，变更立即更新 store。

### Category 模型 — icon 和 color
- `icon` 存储为自由字符串（emoji 字符或图标名），可空。
- `color` 存储为十六进制颜色字符串（如 `#FF6B6B`），可空。
- 前端将 icon 直接渲染为文本，color 渲染为小色块（或 Naive UI `NTag` 带颜色）。
- `CategoryInput`（创建）和 `CategoryUpdateInput`（更新）均包含可选的 `icon` 和 `color` 字段。

### 分类列表排序
- 后端 `list_categories` 查询按 `kind, sort_order, created_at` 排序（替代当前 `kind, created_at`）。
- `reorder_categories` 分配连续的 sort_order 值（0, 1, 2, ...）。前端在排序完成后发送同 kind 下所有分类的新顺序。

## 测试决策

- **好测试的标准**：只测外部行为，不测实现细节。Rust 侧测 update_category 正确更新字段并拒绝非法父分类 kind。前端侧测组件渲染分类、打编辑弹窗、保存时调用正确的 API、拖拽重排反映在 UI 状态中。
- **Rust 单元测试（`commands/categories.rs`）**：扩展现有测试模块：
  - `update_category_updates_fields` — 创建分类后更新 name/icon/color/parent，验证
  - `update_category_rejects_mismatched_kind_parent` — 尝试为支出分类设置收入父分类，预期报错
  - `reorder_categories_reorders` — 创建多个分类后重排，验证 sort_order
  - 已有参照：`create_subcategory_with_parent`、`delete_category_soft_deletes`
- **前端测试（`src/__tests__/` 新文件）**：测试 `CategoryManager.vue` 组件：
  - 按 kind 分组渲染分类，缩进正确
  - 编辑弹窗打开时预填当前值
  - 提交时调用 `api.updateCategory`，载荷正确
  - 删除触发确认后调用 `api.deleteCategory`
  - 已有参照：`CategoryForm.test.ts`（组件挂载 + 交互测试）

## 排除范围

- 跨层级拖拽（通过拖拽改变父分类）— 父分类变更通过编辑弹窗完成。
- 分类的导入/导出。
- 超过 2 层的嵌套。
- 拖拽过程中的动画过渡效果（只做基本的视觉反馈）。
- 同步期间 sort_order 的冲突处理（last-write-wins，沿用现有 sync 模型）。

## 补充说明

- `icon` 和 `color` 字段已存在于 schema 和 `Category` model 中，本 spec 只是让它们在 UI 中可用。
- `sort_order` 迁移是简单的 `ALTER TABLE` 操作；MVP 阶段不涉及向后兼容。
