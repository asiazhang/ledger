//! 定时交易引擎（issue #71）的 BDD 步骤：在命令 seam 上断言
//! - 引擎生成的交易经 transaction::writer 落库，`amount_native_cents` 由
//!   convert_to_native 折算（非硬编码 1:1），缺汇率报错且期次保持可重试；
//! - 分期 / 订阅 / 定时转账生成的类型与金额不回归。
//!
//! 步骤直接调 `scheduled_transactions` 领域函数（即 `commands::scheduled` 的
//! 命令体），与 transactions_steps 直调命令函数的 seam 一致。
//!
//! 步骤按主题拆为子模块（issue #263，纯移动不增删改名任何步骤与断言）：
//! - `create`：创建计划——订阅 / 分期 / 定时转账变体（含无限循环、币种不一致拒绝）
//! - `merchant`：带商户的计划与商户断言（issue #190 / ADR-0028）
//! - `occurrence`：执行期次与引擎落库断言（含汇率夹具、#230 事务自持注入）
//! - `spend`：订阅实际花费口径（issue #160，ADR-0023 决策二）
//! - `plan_edit`：订阅编辑仅非金额字段（issue #162，ADR-0023 决策三）
//! - `plan_detail`：期次详情 / 重试 / 展开（issue #205）
//! - `auto_run`：自动执行（追补）入口（issue #307 / ADR-0042，步骤直调入口注入日期）
//!
//! 跨主题私有 helper 收在 `common`（`execute_occurrence_step`， occurrence /
//! spend / plan_detail 共用）。
//!
//! 本文件经 `e2e.rs` 的 `#[path]` 载入：`#[path]` 模块的子模块默认平铺在文件
//! 所在目录，故各 `mod` 显式 `#[path]` 指入同名子目录 `scheduled_steps/`。

#[path = "scheduled_steps/auto_run.rs"]
mod auto_run;
#[path = "scheduled_steps/common.rs"]
mod common;
#[path = "scheduled_steps/create.rs"]
mod create;
#[path = "scheduled_steps/merchant.rs"]
mod merchant;
#[path = "scheduled_steps/occurrence.rs"]
mod occurrence;
#[path = "scheduled_steps/plan_detail.rs"]
mod plan_detail;
#[path = "scheduled_steps/plan_edit.rs"]
mod plan_edit;
#[path = "scheduled_steps/spend.rs"]
mod spend;
