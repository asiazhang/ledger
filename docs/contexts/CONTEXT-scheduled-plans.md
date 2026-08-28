# 领域词汇表：定时计划

> Ledger 领域词汇表的定时计划分域。全部分域与彼此关系见 `CONTEXT-MAP.md`；决策记录见 `docs/adr/`（本域 MVP 行为决策集中在 ADR-0024）。
> 跨域共享术语（Transaction、Category 等）见核心交易域 `CONTEXT-core.md`，本文不复制定义。
> 若与代码行为冲突，以代码为准并同步修正本文件。

## ScheduledTransaction（定时交易）

- **定义**：一种按照预定规则在将来多次触发生成资金变动的协议/模板。它不是交易本身，而是生成真实交易的规则。
- **边界**：
  - 每次触发时生成一条核心交易域 `Transaction`（交易流水）。
  - 生成的 `Transaction` 是普通的交易记录，参与余额计算、预算统计。
  - 目前涵盖三种业务形态：分期付款（installment）、定期订阅（subscription）、定时转账（scheduled_transfer）。
  - 数据层面采用“核心表 + 扩展表”：通用字段在 `scheduled_transactions`，类型特有字段在 `installment_plans` / `subscription_plans` / `scheduled_transfer_plans`。
- **别名**：不使用“定时任务”（偏技术/调度）、“定时计划”（含糊）、“RecurringPayment”（不能涵盖转账）等词。

## InstallmentPlan（分期计划）

- **定义**：在固定期数内、按固定周期偿还一笔已知总金额的 `ScheduledTransaction`。
- **边界**：
  - 记录总金额 `total_amount_cents` 和总期数 `total_occurrences`。
  - 每期金额由分期金额计算规则得出（MVP 决策见 ADR-0024）。
  - 已还金额和已还期数由 `scheduled_transaction_occurrences` 的 `completed` 状态实时汇总。
  - 每次触发时生成一条核心交易域 `Transaction`（`kind = expense`）。
- **别名**：不使用“loan”、“debt”等词，因为分期不一定是负债（例如分期购买服务）。

## Subscription（订阅）

- **定义**：按周期持续扣款，直到用户手动取消或暂停的 `ScheduledTransaction`。
- **边界**：
  - 每次触发时生成一条核心交易域 `Transaction`（`kind = expense`）。
  - 生命周期 MVP 决策（无结束日期、无最大期数，仅经 `paused` / `cancelled` 停止）见 ADR-0024。
  - 金额固定：**每一期内**金额固定；价格变更策略 MVP 决策（计划金额不可编辑，价格变化 = 取消旧计划 + 按新金额新建，历史在订阅列表中断为两段真实的价格历史；可编辑字段仅限金额以外的非核心字段，编辑只影响未来期次）见 ADR-0023 决策三。
- **别名**：不使用“membership”、“recurring payment”等词，除非业务明确需要区分。

## SubscriptionSpend（订阅花费）

- **定义**：订阅域的花费口径总称，分**实际花费**与**推算成本**两个互不混用的口径。
- **实际花费**：某时间区间（日历月/日历年）内，由订阅计划期次生成的交易流水的实际支出合计；按流水忠实统计，**不摊销**——年付订阅在扣款月全额计入，其余月份为 0。只认现存（未删除）流水，与流水页口径一致，可逐笔对账。计划取消/暂停不影响其历史实际花费。
- **推算成本**：按当前 `active` 订阅计划的参数推算的持续烧钱速度，只算 active 计划、不看执行情况；统一折算为**折算月成本**与**折算年成本**（= 折算月成本 × 12）两个数，不做逐月推算明细。
- **折算月成本系数**：月付 ×1、年付 ÷12、周付 ×52÷12、日付 ×30。
- **边界**：
  - 推算成本只作展示，不落库、不进流水与预算。
  - 分期（installment）与定时转账（scheduled_transfer）不属于订阅花费。
  - 不限定软件类订阅，软件/视频/健身等靠核心交易域分类（Category）区分。
- **别名**：不使用"摊销月费"（摊销口径已否决，实际花费不摊销）、"月均花费"（未区分实际/推算）。

## ScheduledTransfer（定时转账）

- **定义**：在预定日期从用户一个账户向另一个账户转出固定金额的 `ScheduledTransaction`。
- **边界**：
  - 必须指定转出账户和转入账户。
  - 可以是一次性（只执行一期）或周期性（循环执行）。
  - 每次触发时生成一条核心交易域 `Transaction`（`kind = transfer`）。
- **别名**：不使用“auto transfer”、“standing order”等银行术语，除非业务明确需要。

## Occurrence（期次）

- **定义**：`ScheduledTransaction` 的一次应执行实例。每期对应一个触发日期，可能生成一条核心交易域 `Transaction`。
- **边界**：
  - 已发生的期次必须实例化落库，记录执行状态和生成的交易 ID。
  - 未来期次只预生成有限窗口（如未来 N 期或 N 个月），远期按需展开。
  - 单期可独立查看、重试，不破坏整个计划。
  - 数据层面统一使用 `scheduled_transaction_occurrences` 表。
- **状态**：`pending`（待执行）、`processing`（执行中）、`completed`（已完成）、`failed`（失败）、`skipped`（已跳过）、`cancelled`（已取消）。
- **别名**：不使用“任务实例”、“执行记录”等偏技术词汇。

## Plan Lifecycle（计划生命周期）

- **定义**：`ScheduledTransaction` 的计划状态集合（`active` / `paused` / `cancelled` / `completed`）与状态变更规则，决定新期次的生成与既有期次/交易的去留。
- **MVP 决策**：状态集合、取消/暂停副作用（取消不删已生成交易、暂停不改已生成期次）与不支持项（单独取消/跳过某期、修改单期金额或日期）见 ADR-0024。

## Timing（时间精度）

- **日期精度**：所有定时交易只精确到日期，不记录具体执行时间。
- **执行日期**：`Occurrence` 的 `scheduled_date` 为 ISO 8601 日期格式（YYYY-MM-DD）。
- **节假日处理**：MVP 采用严格日期（不顺延）的决策见 ADR-0024。
- **边界**：核心交易域 `Transaction.date` 直接复用 `Occurrence` 的 `scheduled_date`，两者保持一致。

## Counterparty（交易对手）

- **定义**：定时交易中的收款方或付款对象，例如商家、贷款机构、订阅服务商。
- **MVP 决策**：`counterparty` 字段落点（计划扩展表）、生成交易的复制方式、不在核心交易域 `Transaction` 表新增通用字段及 `ScheduledTransfer` 不使用 `counterparty`（用 `to_account_id` 表示本方账户间转账）的决策与边界见 ADR-0024。

## Recurrence Rule（周期规则）

- **定义**：定时交易用显式字段（`recurrence_type` 周期类型 / `recurrence_interval` 间隔 / `recurrence_day` 具体日期或星期）表达的周期，如每 1 个月、每周一。
- **MVP 决策**：显式字段而非 RRULE、仅支持常见固定周期（复杂规则留到后续版本）见 ADR-0024。

## Failure Policy（失败策略）

- **定义**：期次执行失败即标记为 `failed`、由用户手动重试的处理约定。
- **MVP 决策**：不自动重试、不自动跳过、不产生滞纳金及其理由见 ADR-0024。
