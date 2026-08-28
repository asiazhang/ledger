# ADR 0024: 定时交易 MVP 行为决策（生命周期 / 分期金额 / 周期规则 / 时间精度 / 交易对手 / 失败策略）

- 状态：已接受
- 日期：2026-08-28
- 作者：Ledger 项目

## 背景

定时交易域（`ScheduledTransaction` + `Occurrence`，见 ADR-0001 / ADR-0003）在 MVP 设计期确定了一批行为决策：计划生命周期、分期金额计算、周期规则、时间精度、交易对手落点、失败策略。这些决策原先以「MVP 决策」段落形式记录在 CONTEXT.md 词汇表条目（Plan Lifecycle、Amount Model、Subscription、Recurrence Rule、Timing、Counterparty、Failure Policy）中。

随着文档结构调整（issue #163 / #164），词汇表收敛为纯定义、决策叙述统一迁入 ADR。本文是这批决策的新家：以下各节内容为**原样搬运**，未新增、未改写任何决策。

## 决策

### 计划生命周期（Plan Lifecycle）

- **MVP 决策**：`ScheduledTransaction` 支持以下状态变更：
  - `active`（正常执行）
  - `paused`（暂停，不再生成新期次）
  - `cancelled`（取消，所有未执行期次状态变为 `cancelled`）
  - `completed`（计划自然完成，所有期次已执行）
- **MVP 不支持**：单独取消/跳过某期、修改单期金额或日期。
- **边界**：
  - 取消整个计划不会删除已生成的 `Transaction`。
  - 暂停/恢复不改变已生成的期次或交易。

### 订阅生命周期（Subscription）

- MVP 阶段没有结束日期，也没有最大期数限制。
- 只能通过 `paused` 或 `cancelled` 状态停止。
- 金额固定：**每一期内**金额固定。

订阅价格变更策略（计划金额不可编辑、价格变化 = 取消旧计划 + 按新金额新建、可编辑字段仅限金额以外）见 ADR-0023 决策三，本文不重复。

### 分期金额计算规则（ScheduledTransaction）

- **MVP 决策**：每期金额固定，使用 `ScheduledTransaction` 的 `amount_cents` 字段。
- **分期金额计算规则**：
  1. `InstallmentPlan` 记录 `total_amount_cents` 和 `total_occurrences`。
  2. 每期基准金额 = `floor(total_amount_cents / total_occurrences)`。
  3. 剩余尾差 = `total_amount_cents - base_amount_cents * total_occurrences`。
  4. 最后一期金额 = `base_amount_cents + 剩余尾差`。
  5. 其余每期金额 = `base_amount_cents`。
- **边界**：MVP 不支持每期金额不同；不支持 subscription 中途涨价。

### 周期规则（Recurrence Rule）

- **MVP 决策**：使用显式字段表达周期，不引入 RRULE 等通用表达式。
- **字段**：
  - `recurrence_type`：周期类型，如 `daily`、`weekly`、`monthly`、`yearly`。
  - `recurrence_interval`：间隔，如每 1 个月、每 2 周。
  - `recurrence_day`：具体日期/星期，如每月 1 日、每周一。
- **边界**：MVP 只支持常见固定周期；复杂规则（如“每月最后一个工作日”）留到后续版本。

### 时间精度（Timing）

- **节假日处理**：MVP 采用严格日期，不因为周末/节假日顺延。

（日期精度、ISO 8601 日期格式与 `Transaction.date` 复用 `scheduled_date` 的口径仍见 CONTEXT.md「Timing（时间精度）」条目。）

### 交易对手（Counterparty）

- **MVP 决策**：在 `InstallmentPlan` 和 `Subscription` 的扩展表中记录 `counterparty` 字段；生成 `Transaction` 时复制到 `Transaction.note` 或作为展示字段。
- **MVP 不扩展**：不在 `Transaction` 表中新增通用 `counterparty` 字段，避免改动现有核心表。
- **边界**：`ScheduledTransfer` 不使用 `counterparty`，而是使用 `to_account_id` 表示本方账户间转账。

### 失败策略（Failure Policy）

- **MVP 决策**：MVP 阶段只支持“失败即标记为 failed，由用户手动重试”。不自动重试、不自动跳过、不产生滞纳金。
- **理由**：离线优先场景下，自动重试容易在多设备间产生重复执行；手动重试让用户明确控制资金流出，适合个人账本。

## 理由

除失败策略一节自带原记录的理由外，其余决策在 MVP 设计期作为范围取舍确定，未单独留下成文理由；相关背景可参见 ADR-0001（单期状态管理、失败与重试可控）与 ADR-0023（订阅金额不可编辑的理由）。

## 代价与边界

- 「不支持单独取消/跳过某期、修改单期金额或日期」「只支持常见固定周期」「严格日期不顺延」「失败手动重试」均为已知 MVP 取舍，不是缺陷。
- 放宽其中任何一条（如自动重试、单期跳过、复杂周期规则）都须先修订本 ADR，并同步对应词汇表条目。

## 相关 ADR

- ADR-0001 / ADR-0003：定时交易的数据模型与核心表 + 扩展表结构。
- ADR-0011：金额模型与交易类型映射（`ScheduledTransaction → Transaction` 的 kind 生成映射见 CONTEXT.md「Transaction Kind Mapping」）。
- ADR-0023：订阅花费双口径与订阅金额不可编辑（决策三）。
