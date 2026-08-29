# 领域词汇表：预算

> Ledger 领域词汇表的预算分域（单列小域）。全部分域与彼此关系见 `CONTEXT-MAP.md`；决策记录见 `docs/adr/`（ADR-0029 永久滚动预算）。
> 跨域共享术语（Transaction、Amount Model、Transaction Kind Mapping、Category 等）见核心交易域 `CONTEXT-core.md`，本文不复制定义；spent 口径引用核心交易域 Amount Model，不在此重复。
> 若与代码行为冲突，以代码为准并同步修正本文件。

## Budget（预算）

- **定义**：用户对某一**支出分类**设定的周期性支出上限，一经设置**永久滚动**生效（ADR-0029）——不设起止日期、不逐期排程，进度窗口永远是「当前自然周期」（见 BudgetPeriod 与 BudgetProgress）。表 `budgets`，真源 `src-tauri/src/models/budget.rs`。
- **边界**：
  - **「分类 + 周期」唯一**：同分类同周期仅允许一条未删除预算；重复创建明确拒绝并引导编辑已有预算，**不静默覆盖**（写入行为细节见 ADR-0029 决策 4 与模型文档 `docs/model/basic/budgets.md`）；软删后可重新创建。
  - **只能挂支出分类**：收入分类与不存在的分类均被拒绝（注意：`categories.kind` 闭集仅 `income | expense`，与 BudgetPeriod 是两个无关概念）。
  - **金额为正数**：`amount_cents` 整数分存储，0 或负数拒绝。
  - **编辑仅允许改金额**（`update_budget`）：分类/周期不可改，改法为删旧建新；金额与支出分类校验复用创建侧逻辑，沿用软删除同一套 updated_at/version/device_id 更新机制。
  - **`start_date` 为冻结残留**：已发布 schema 只增不改（该列自 v0.1.0 初始 schema 即存在），列保留但不参与任何计算，仅创建时记录用途；UI 无日期选择器、列表无「开始日期」列（ADR-0029）。
  - 分类删除受限（`ON DELETE RESTRICT`），防止有预算的分类被误删。
- **别名**：不使用「限额」「月度计划」（「计划」与定时计划域 ScheduledTransaction 混淆）。

## BudgetPeriod（预算周期）

- **定义**：预算的周期粒度，**闭集二值**：`monthly`（按月）/ `yearly`（按年），真源 `models::budget::BudgetPeriod` 枚举（serde snake_case，未知值拒绝）。
- **边界**：
  - 周期**决定进度窗口**：`monthly` = 今天所在自然月，`yearly` = 今天所在自然年（见 BudgetProgress）；窗口随时间自动滚动，无需任何用户操作（永久滚动，ADR-0029）。
  - 周期参与「分类 + 周期」唯一性约束：同一分类可同时各设一条月预算与年预算。
  - **不可编辑**：创建后周期不可改，改法为删旧建新（见 Budget）。
- **别名**：不使用「频率」（与定时计划域 Recurrence Rule 的周期语义划清边界——预算周期只是窗口粒度，不驱动任何事件触发）。

## BudgetProgress（预算进度）

- **定义**：某 Budget 在**当前自然周期窗口**内的消耗情况，= 预算行（附 `category_name`）+ `spent_cents` + `over_budget`；由 `budget_progress` 命令实时计算，不持久化。
- **边界**：
  - **spent 口径 = 核心交易域 Amount Model 的 `ExpenseNet` 度量**（见 Transaction Kind Mapping）：与报表分类净值同口径，不另造第二个口径；投资类 kind（buy/sell 等）不参与。
  - **统计范围含子分类**：子分类支出并入父分类预算统计。
  - **窗口与 `start_date` 无关**：窗口由命令层注入的本地「今天」推导——`monthly` = 今天所在自然月，`yearly` = 今天所在自然年；随时间自动滚动，存量旧日期预算行零迁移生效（ADR-0029）。
  - **退款冲减消耗**：窗口内退款使 spent 回落，甚至可为负（当期退款多于当期支出，如原支出发生在上期）。
  - `over_budget` = `spent_cents > amount_cents`。
  - 首页预算进度卡与预算页复用同一命令，零改动自动跟随滚动窗口。
- **别名**：不使用「使用率」「消耗率」（本域只输出金额差值判断，不输出百分比口径）。
