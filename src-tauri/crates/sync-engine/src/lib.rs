// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块与外挂 tests/）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。
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

//! 多端同步域（issue #855 / #856 / ADR-0091）：OpLog 基座与双端合并语义——
//! 同步元数据落库、op 产出信封、跨端全序、幂等重放、LWW 合并、OccurrenceKey
//! 防双扣与 ParkedOp 挂起队列的完整闭环。
//!
//! 接缝：
//! - [`engine`]（同步引擎公开接口）：[`engine::apply_ops`]（幂等重放）、
//!   [`engine::ingest_ops`]（wire 接入，解析失败挂起）、[`engine::read_ops`]
//!   （日志读取，全序返回）、[`engine::total_order`]（跨端全序）、
//!   [`engine::ops_after_positions`]（位点之后的增量）、
//!   [`engine::parked_ops`]（挂起队列清单）——本域行为的唯一断言权威层（域单测）。
//! - [`checkpoint`]：Checkpoint 产出（全量快照 + 位点）、新端引导与截断机制
//!   （issue #857，v1 永不截断、机制默认不启用）。
//! - [`channel`]：通道层（issue #859）——哑字节通道上的目录布局与 manifest、
//!   同步轮次（发布自己流 / 拉取他人流）、SyncEnvelope 封包（[`envelope`]）、
//!   Checkpoint 通道传递与新端取件。
//! - [`trigger`]：触发编排（issue #863）——通道配置与构库单点、轮次编排与挂起
//!   通知数据面、打开应用即同步与运行期低频轮询、会话密钥形态判定的信封模式。
//! - [`transport`]：Transport 哑字节通道抽象（v1 唯一后端是 S3 兼容对象存储，
//!   两端同一代码路径；WebDAV 已随 #1221 退役）。
//! - 位点与设备标识、op 行落库/读取（`sync_stream_positions` / `sync_device` /
//!   `sync_ops` 表的唯一 SQL 收口）自 #1089 起下放协议 crate
//!   `ledger-sync-protocol`（rank 0，业务域与 sync_engine 共同底座）；本域经
//!   适配层（[`ops`]）承载域信封的载荷知识（serde 往返）。
//! - [`ops`]：op 行落库与读取的适配层（域信封 ↔ JSON；SQL 收口在协议 crate）。
//! - [`command`]：跨端语义命令信封与重放契约（DomainCommand / ReplayBinding，
//!   只增不改）——同步域对业务域暴露的契约面（ADR-0101 决策 4b）；重放效果
//!   ReplayEffect 与命令契约 SyncCommand 已下放协议 crate（#1089）。
//! - [`registry`]：重放注册表（ADR-0101）——14 个语义命令类型的适配绑定与
//!   `DomainCommand::subject` 组装臂，与 ops/parked 平级。
//! - [`model`]：op 信封 wire 模型（[`model::SyncOp`]）。
//!
//! 复制模型（ADR-0091 决策 2/3）：op 载荷是语义级域命令，重放经既有写入接缝
//! （交易行为编排入口的命令执行形态，见 `ledger_transaction` 的
//! `replay_command`）执行，不绕过不变量；折算在记账端一次性完成并随 op 携带
//! （源端折算），重放不依赖本地汇率表状态与设备配置。派生数据（余额缓存）
//! 不进日志，各端经既有接缝重算（ADR-0067 延伸）。合并语义（ADR-0091 决策
//! 4/5/6，issue #856）：同实体并发编辑按全序取序末者（LWW）；期次触发经
//! OccurrenceKey 确定性落地身份防双扣；不可重放 op 进挂起队列并码化报错，
//! 不阻塞其余重放，不静默丢弃。
//!
//! 依赖方向（spec #1086 / issue #1107 AC）：本 crate 是域层最高点的业务域
//! crate——消费**基础设施**（`db` 连接与事务原语 / `error` 码化错误 / `fs_util`
//! 快照文件工具 / `settings` 设置单点 / `signals` 失效信号）、**同步协议**
//!（op 行落库与读取、位点、设备标识、命令契约 SyncCommand/ReplayEffect）与
//! **全部业务域**（重放注册表把 14 个语义命令绑定回各域写入接缝：核心交易、
//! 定时计划、账户、分类、商户、币种、预算、保单、物品、实物资产、投资；
//! 轮次持锁复用备份域的连接锁助手 `lock_conn_with_timeout`）——域→域均为
//! 上层域消费下层域的合法直呼（ADR-0112 决策 2）。任何业务域对本域零依赖
//!（业务域→同步域零容忍，ADR-0101 决策 4b / #1089 收紧：同步协议面已下放
//! 协议 crate），对根包（壳层）零生产依赖，反向引用由 cargo 依赖图编译期拒绝
//!（生产依赖面无根包，dev-dependency 环只覆盖测试目标）。
//!
//! 兼容面（ADR-0112 决策 3「调用点零改动」）：根包以
//! `pub use ledger_sync_engine as sync_engine;` 再导出保留原引用路径——壳层
//! `commands::sync_channel`、`commands::boot` / `commands::encryption` 的会话
//! 信封接线、`lib.rs` 后台服务编排点（`start_triggers`）、`test_support` 测试
//! 工厂接线、e2e 与集成测试的 `crate::sync_engine::…` /
//! `tauri_app_lib::sync_engine::…` 引用零改动。

pub mod channel;
pub mod checkpoint;
pub mod command;
pub mod engine;
pub mod envelope;
pub mod model;
pub mod ops;
pub mod parked;
pub mod registry;
pub mod transport;
pub mod trigger;

pub use channel::{
    ChannelLayout, ChannelManifest, ChannelOptions, CheckpointPointer, FetchedCheckpoint,
    SegmentEntry, StreamManifest, SyncRoundReport, fetch_checkpoint, publish_checkpoint,
    publish_checkpoint_with, run_round, run_round_with,
};
pub use checkpoint::{
    BootstrapOutcome, Checkpoint, bootstrap_from_channel, bootstrap_from_checkpoint,
    create_checkpoint, truncate_stream_before,
};
pub use command::DomainCommand;
pub use engine::{
    ApplyReport, OpOutcome, apply_ops, ingest_ops, ops_after_positions, parked_ops, read_ops,
    stream_positions, total_order,
};
pub use envelope::EnvelopeMode;
pub use ledger_sync_protocol::position::StreamPosition;
pub use model::SyncOp;
pub use parked::ParkedOp;
pub use transport::{
    Transport,
    s3::{S3Config, S3Transport},
};
pub use trigger::{
    SessionEnvelope, SyncChannel, SyncChannelConfig, TriggerTimings, build_channel,
    configured_channel, probe_channel, run_auto_round, run_round_once, start_sync_scheduler,
    start_sync_scheduler_with, start_triggers, sync_after_write, sync_on_start,
};

#[cfg(test)]
mod tests;
