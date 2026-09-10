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
//! - [`transport`]：Transport 哑字节通道抽象（v1 内置 WebDAV 后端，两端同一
//!   代码路径）。
//! - [`positions`]：位点（各来源流已应用水位，`sync_stream_positions` 表的
//!   唯一 SQL 收口）。
//! - [`device`]：DeviceId 读取（首用生成并持久化）与端内单调逻辑时钟分配。
//! - [`ops`]：op 行落库与读取（`sync_ops` 表的唯一 SQL 收口）。
//! - [`parked`]：挂起队列（`sync_parked_ops` 表的唯一 SQL 收口）。
//! - [`command`]：跨端语义命令信封（DomainCommand，只增不改）。
//! - [`model`]：op 信封 wire 模型（[`model::SyncOp`]）。
//!
//! 复制模型（ADR-0091 决策 2/3）：op 载荷是语义级域命令，重放经既有写入接缝
//! （交易行为编排入口的命令执行形态，见 `crate::transaction` 的
//! `replay_command`）执行，不绕过不变量；折算在记账端一次性完成并随 op 携带
//! （源端折算），重放不依赖本地汇率表状态与设备配置。派生数据（余额缓存）
//! 不进日志，各端经既有接缝重算（ADR-0067 延伸）。合并语义（ADR-0091 决策
//! 4/5/6，issue #856）：同实体并发编辑按全序取序末者（LWW）；期次触发经
//! OccurrenceKey 确定性落地身份防双扣；不可重放 op 进挂起队列并码化报错，
//! 不阻塞其余重放，不静默丢弃。
//!
//! 依赖方向恒为「壳层 → sync_engine → 基础设施」；对交易域为横向消费（重放
//! 分派与其嵌套感知事务原语），域间横向消费是既有事实（先例：investment ↔
//! transaction、scheduled_transactions → transaction）。

pub mod channel;
pub mod checkpoint;
pub mod command;
pub mod device;
pub mod engine;
pub mod envelope;
pub mod model;
pub mod ops;
pub mod parked;
pub mod positions;
pub mod transport;

/// 域内共享接缝（crate 内消费）：DeviceId 读取与本地 op 产出信封。
pub(crate) use device::device_id;
pub(crate) use ops::record_local;

pub use channel::{
    ChannelLayout, ChannelManifest, ChannelOptions, CheckpointPointer, SegmentEntry,
    StreamManifest, SyncRoundReport, fetch_checkpoint, publish_checkpoint, publish_checkpoint_with,
    run_round, run_round_with,
};
pub use checkpoint::{
    Checkpoint, bootstrap_from_checkpoint, create_checkpoint, truncate_stream_before,
};
pub use command::DomainCommand;
pub use engine::{
    ApplyReport, OpOutcome, apply_ops, ingest_ops, ops_after_positions, parked_ops, read_ops,
    stream_positions, total_order,
};
pub use envelope::EnvelopeMode;
pub use model::SyncOp;
pub use parked::ParkedOp;
pub use positions::StreamPosition;
pub use transport::{
    Transport,
    webdav::{WebDavConfig, WebDavTransport},
};

#[cfg(test)]
mod tests;
