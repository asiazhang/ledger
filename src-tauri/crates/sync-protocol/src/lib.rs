// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// 经 crate 根 cfg(test) 整体放行，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

//! 同步协议 crate（spec #1086 / issue #1089）：多端同步的**最底层共享协议**——
//! 设备标识、领域命令契约、op 本地记录与读取、流位点。
//!
//! 层位（ADR-0056 延伸，#1089）：协议层是 workspace 依赖图的**共享底座**——
//! 全部业务域与多端同步域（`sync_engine`）的共同底层，自身只依赖基础设施
//!（[`ledger_infra`]，其下再无更底层）与数据面惯用库。依赖方向恒为「业务域 /
//! sync_engine → 本 crate → 基础设施」；任何业务域类型对本 crate 不可见，反向
//! 引用即环，由 cargo 依赖图在编译期拒绝：
//!
//! ```compile_fail
//! // 业务域/壳层类型对本 crate 不可见：协议 crate 不反向依赖业务域与壳层
//! //（反向引用即环，spec #1086 裁决）。tauri_app_lib 不在本 crate 依赖面，
//! // 本用例编译必红——依赖方向由 cargo 依赖图强制，非约定自律。
//! use tauri_app_lib::transaction::TransactionCommand;
//! ```
//!
//! 承载物（自 sync_engine 下放，#1089）：
//! - [`command`]：同步命令契约 [`command::SyncCommand`]（实体标签 + 可空实体键）
//!   与重放效果 [`command::ReplayEffect`]——业务域命令类型对协议面的自述；
//! - [`device`]：DeviceId 首用生成并持久化、端内单调逻辑时钟（`sync_device`
//!   单行的唯一 SQL 收口）；
//! - [`op`]：op 行落库与读取（`sync_ops` 表的唯一 SQL 收口）——本地 op 产出
//!   [`op::record_local`]（泛型于 [`command::SyncCommand`]，信封组装、位点推进
//!   与写后通知）与裸部件落库 [`op::insert_row`]（重放路径），以及幂等/LWW/截断
//!   的日志查询面；写后钩子（ADR-0091 决策 9 的信号点）也住此——登记点反转，
//!   由同步调度在启动时装入响应闭包；
//! - [`position`]：流位点（`sync_stream_positions` 表的唯一 SQL 收口，issue
//!   #857）——连续前缀水位的推进与查询。
//!
//! 域载荷与 `DomainCommand` 信封不进本 crate：域 payload 内嵌域模型类型，属
//! 业务域知识；信封枚举住 sync_engine（对业务域的契约面不变，ADR-0101 决策
//! 4b）。本 crate 只认「实体标签 + 实体键 + serde 载荷」的协议形状。

pub mod command;
pub mod device;
pub mod op;
pub mod position;
