//! 跨端语义命令信封与重放契约（DomainCommand，ADR-0091 决策 2 / ADR-0101）：
//! op 的载荷形态、重放效果与重放绑定契约。
//!
//! 与既有写入接缝同语义的域级命令，按实体分派；重放端经同一编排协议执行，
//! 不绕过不变量。**只增不改**：新增实体/字段只追加（旧日志可在新 schema 上
//! 重放），与已发布契约纪律同构；`entity` tag 与 `sync_ops.entity` 列同源。
//!
//! 本模块是同步域对业务域暴露的**契约面**（ADR-0101 决策 1/4b）：信封
//! [`DomainCommand`]、重放效果 [`ReplayEffect`] 与重放绑定契约 [`ReplayBinding`]
//! 集中住此——业务域只许经本模块路径与根再导出白名单引用同步域（门 b，
//! `check-structure.ts`）。14 个适配绑定与 `DomainCommand::subject` 的组装臂
//! 住 [`super::registry`]（契约读一处即知，绑定与组装同居一处）。

use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::accounts::AccountCommand;
use crate::budget::BudgetCommand;
use crate::categories::CategoryCommand;
use crate::currencies::LedgerSettingCommand;
use crate::error::Result;
use crate::investment::{ExchangeRateCommand, InstrumentCommand, PriceCommand};
use crate::item::ItemCommand;
use crate::merchants::MerchantCommand;
use crate::physical_asset::PhysicalAssetCommand;
use crate::policy::{InsurerCommand, PolicyCommand};
use crate::scheduled_transactions::ScheduledCommand;
use crate::transaction::TransactionCommand;

/// 语义命令（按实体分派；`entity` tag 与 `sync_ops.entity` 列同源）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "entity", content = "payload", rename_all = "snake_case")]
pub enum DomainCommand {
    /// 交易创建/修改/删除命令（核心交易域）。
    Transaction(TransactionCommand),
    /// 定时计划域命令（期次触发、计划 CRUD 与期次展开；ADR-0091 决策 5，#860）。
    Scheduled(ScheduledCommand),
    /// 账本级设置命令（LedgerLevelSetting，issue #858 / ADR-0091 决策 3）。
    LedgerSetting(LedgerSettingCommand),
    /// 账户命令（参考数据字典，issue #860）。
    Account(AccountCommand),
    /// 分类命令（参考数据字典，issue #860）。
    Category(CategoryCommand),
    /// 商户命令（参考数据字典，issue #860）。
    Merchant(MerchantCommand),
    /// 预算命令（issue #860）。
    Budget(BudgetCommand),
    /// 保单命令（保险域，issue #860）。
    Policy(PolicyCommand),
    /// 保司字典命令（保险域自有字典，issue #860）。
    Insurer(InsurerCommand),
    /// 物品命令（issue #860）。
    Item(ItemCommand),
    /// 实物资产命令（issue #860）。
    PhysicalAsset(PhysicalAssetCommand),
    /// 标的字典命令（投资域，issue #861）。
    Instrument(InstrumentCommand),
    /// 汇率命令（投资域，issue #861）。
    ExchangeRate(ExchangeRateCommand),
    /// 用户侧价格命令（现价录入 / 手动报价，issue #861；东财行情不进 op）。
    Price(PriceCommand),
}

/// 单条命令的重放执行效果（ADR-0101 决策 3）。
///
/// 13 个域侧重放入口返回 `Result<()>`，由适配绑定映射为 [`ReplayEffect::Applied`]；
/// 期次触发（OccurrenceKey 独有语义，见 CONTEXT-sync 期次词条）由域侧原样透传
/// [`ReplayEffect::IdempotentHit`]——逼其余 13 个无此概念的域返回它只会让类型说谎，
/// 差异留在适配层比假统一诚实。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplayEffect {
    /// 落地新效果。
    Applied,
    /// 幂等命中：该效果已在本端存在。
    IdempotentHit,
}

/// 语义命令重放绑定契约（ADR-0101 决策 1）：一个语义命令类型一条绑定，单点承载
/// 「实体标签 + 裁决域派生 + 重放入口」；域侧接缝的 6 个函数名与 2 种返回形状由
/// 绑定吸收，域侧除裁决键派生外零改动。
///
/// 可重放契约五条（ADR-0091）：① 载荷可 serde 且只增不改、⑤ 确定性——由
/// [`DomainCommand`] 信封与域侧命令类型承载；② 裁决域派生——[`Self::subject`]；
/// ③ 域暴露重放入口——[`Self::replay`]；④ 重放不产本地 op——域侧不认识同步域
/// 内部件，由结构守门钉死（门 b，`check-structure.ts`）。
pub(crate) trait ReplayBinding {
    /// 实体标签（serde tag 与 `sync_ops.entity` 列同源；门 a 样本轮询断言之锚）。
    const ENTITY: &'static str;
    /// 本绑定承载的语义命令类型。
    type Command;
    /// 命令指向的实体键。`None` = 无实体指向（冲突域在 OccurrenceKey，不参与
    /// 同实体 LWW，ADR-0091 决策 5）。标签不在此返回——由注册表单源组装
    /// （标签一律取 [`Self::ENTITY`]，ADR-0101 勘误 3）。
    fn subject(command: &Self::Command) -> Option<Cow<'_, str>>;
    /// 重放入口：转发域侧既有写入接缝（守卫原样生效，不产出本地 op）。
    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect>;
}
