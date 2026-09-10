//! 跨端语义命令信封（DomainCommand，ADR-0091 决策 2）：op 的载荷形态。
//!
//! 与既有写入接缝同语义的域级命令，按实体分派；重放端经同一编排协议执行，
//! 不绕过不变量。**只增不改**：新增实体/字段只追加（旧日志可在新 schema 上
//! 重放），与已发布契约纪律同构；`entity` tag 与 `sync_ops.entity` 列同源。

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::accounts::AccountCommand;
use crate::budget::BudgetCommand;
use crate::categories::CategoryCommand;
use crate::currencies::LedgerSettingCommand;
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

impl DomainCommand {
    /// 实体判别键（`sync_ops.entity` 列取值；与 serde tag 同源，防双源漂移）。
    pub fn entity(&self) -> &'static str {
        match self {
            DomainCommand::Transaction(_) => "transaction",
            DomainCommand::Scheduled(_) => "scheduled",
            DomainCommand::LedgerSetting(_) => "ledger_setting",
            DomainCommand::Account(_) => "account",
            DomainCommand::Category(_) => "category",
            DomainCommand::Merchant(_) => "merchant",
            DomainCommand::Budget(_) => "budget",
            DomainCommand::Policy(_) => "policy",
            DomainCommand::Insurer(_) => "insurer",
            DomainCommand::Item(_) => "item",
            DomainCommand::PhysicalAsset(_) => "physical_asset",
            DomainCommand::Instrument(_) => "instrument",
            DomainCommand::ExchangeRate(_) => "exchange_rate",
            DomainCommand::Price(_) => "price",
        }
    }

    /// 命令指向的实体（实体判别键，实体 id）：LWW 裁决域（同实体并发编辑取
    /// 全序末者，ADR-0091 决策 4）。无实体指向的命令返回 None——其冲突域另行
    /// 裁定（如期次触发命令的 OccurrenceKey，ADR-0091 决策 5）。裁决键为
    /// `Cow`：自然键派生型命令（货币对、标的 × 周）以派生键为域，与落库冲突
    /// 键同粒度（issue #861）。
    pub fn subject(&self) -> Option<(&'static str, Cow<'_, str>)> {
        match self {
            DomainCommand::Transaction(cmd) => {
                Some(("transaction", Cow::Borrowed(cmd.subject_id())))
            }
            // 定时计划：计划 CRUD 按 plan 实体 LWW；期次触发冲突域在 OccurrenceKey、
            // 期次展开按本端现状重推，均无实体指向（ADR-0091 决策 4/5）。
            DomainCommand::Scheduled(cmd) => cmd
                .subject()
                .map(|(entity, id)| (entity, Cow::Borrowed(id))),
            DomainCommand::LedgerSetting(cmd) => {
                Some(("ledger_setting", Cow::Borrowed(cmd.subject_id())))
            }
            DomainCommand::Account(cmd) => Some(("account", Cow::Borrowed(cmd.subject_id()))),
            DomainCommand::Category(cmd) => cmd
                .subject()
                .map(|(entity, id)| (entity, Cow::Borrowed(id))),
            DomainCommand::Merchant(cmd) => Some(("merchant", Cow::Borrowed(cmd.subject_id()))),
            DomainCommand::Budget(cmd) => Some(("budget", Cow::Borrowed(cmd.subject_id()))),
            DomainCommand::Policy(cmd) => Some(("policy", Cow::Borrowed(cmd.subject_id()))),
            DomainCommand::Insurer(cmd) => Some(("insurer", Cow::Borrowed(cmd.subject_id()))),
            DomainCommand::Item(cmd) => Some(("item", Cow::Borrowed(cmd.subject_id()))),
            DomainCommand::PhysicalAsset(cmd) => cmd
                .subject()
                .map(|(entity, id)| (entity, Cow::Borrowed(id))),
            DomainCommand::Instrument(cmd) => cmd.subject(),
            DomainCommand::ExchangeRate(cmd) => cmd.subject(),
            DomainCommand::Price(cmd) => cmd.subject(),
        }
    }
}
