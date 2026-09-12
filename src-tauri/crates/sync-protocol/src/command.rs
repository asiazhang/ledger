//! 同步命令契约与重放效果（自 sync_engine/command.rs 下放，issue #1089）：
//! 命令类型对协议面的自述——实体标签、可空实体键与 serde 载荷形状。
//!
//! 协议 crate 只认「实体标签 + 实体键 + serde 载荷」的形状，不认识任何具体
//! 域命令类型：`DomainCommand` 信封与 14 个域 payload 留在业务域与 sync_engine
//! （ADR-0101 决策 4b），各自实现 [`SyncCommand`] 向协议面自述。op 落库（实体
//! 列取值、信封组装）与重放绑定（标签单源）都消费本契约。
//!
//! 只增不改：契约演进只追加（旧日志可在新 schema 上重放），与已发布契约纪律
//! 同构；`ENTITY` 与 `sync_ops.entity` 列、serde 信封 tag 同源。

use std::borrow::Cow;

/// 同步命令契约（op 载荷的最小协议形状）：一个语义命令类型一条实现。
///
/// - [`Self::ENTITY`]：实体标签——serde 信封 tag 与 `sync_ops.entity` 列同源
///   （标签单源，ADR-0101 勘误 3；注册表绑定与信封组装都取此处）；
/// - [`Self::subject`]：命令指向的实体键。`None` = 无实体指向（冲突域在
///   OccurrenceKey，不参与同实体 LWW，ADR-0091 决策 5）。
pub trait SyncCommand: serde::Serialize {
    /// 实体标签（serde 信封 tag 与 `sync_ops.entity` 列同源）。
    const ENTITY: &'static str;
    /// 命令指向的实体键；`None` = 无实体指向（不参与同实体 LWW）。
    fn subject(&self) -> Option<Cow<'_, str>>;
}

/// 单条命令的重放执行效果（ADR-0101 决策 3，自 sync_engine 下放：重放绑定与
/// 期次触发域共用此类型，协议 crate 是共同底座）。
///
/// 13 个域侧重放入口返回 `Result<()>`，由适配绑定映射为 [`ReplayEffect::Applied`]；
/// 期次触发（OccurrenceKey 独有语义，见 CONTEXT-sync 期次词条）由域侧原样透传
/// [`ReplayEffect::IdempotentHit`]——逼其余 13 个无此概念的域返回它只会让类型说谎，
/// 差异留在适配层比假统一诚实。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayEffect {
    /// 落地新效果。
    Applied,
    /// 幂等命中：该效果已在本端存在。
    IdempotentHit,
}
