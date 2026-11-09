//! 同步命令信封载荷（共享语义区，issue #855 / ADR-0091）：`TransactionCommand`。
//!
//! 职责：创建/修改携带实体 id + 归一化行、删除只需实体 id 的 payload 形态，以及
//! `SyncCommand` 协议面实现。不变量：**只增不改**，旧日志可在新 schema 重放；
//! `action` serde tag 与协议实体标签同源（门 a 二源断言之锚）。ADR 指针：ADR-0091。
//! 陷阱：op 产出单点在写路径 `crate::write::op`，本模块不落库。

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use ledger_sync_protocol::command::SyncCommand;

use super::fields::{ConvertCommandFields, InvestmentCommandFields, SplitCommandFields};
use crate::model::NormalizedTransaction;

/// 交易同步命令（serde：`action` 判别；作为 DomainCommand 信封的 payload 内嵌）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TransactionCommand {
    /// 创建交易：实体 id 与归一化行随命令携带（重放端不得重新生成 id 或折算）。
    Create {
        id: String,
        row: NormalizedTransaction,
        /// 投资 kind（buy/sell/dividend）的语义字段；其余 kind 为 None。
        investment: Option<InvestmentCommandFields>,
        /// 转换 kind（convert）的语义字段与源端算定的结转成本；其余 kind 与
        /// 旧版本设备产出的载荷（该成员缺省）为 None（ADR-0099 决策 6）。
        #[serde(default)]
        convert: Option<ConvertCommandFields>,
        /// 份额调整 kind（split）的语义字段与源端比对锚点；其余 kind 与旧版本
        /// 设备产出的载荷（该成员缺省）为 None（ADR-0106 决策 9 / issue #1053）。
        #[serde(default)]
        split: Option<SplitCommandFields>,
    },
    /// 全字段替换修改（与本地修改同语义）：实体 id 与归一化后的新行随命令携带。
    Update {
        id: String,
        row: NormalizedTransaction,
        /// 投资 kind（buy/sell/dividend）的语义字段；其余 kind 为 None。
        investment: Option<InvestmentCommandFields>,
        /// 转换 kind（convert）的语义字段与源端算定的结转成本；其余 kind 与
        /// 旧版本设备产出的载荷（该成员缺省）为 None（ADR-0099 决策 6）。
        #[serde(default)]
        convert: Option<ConvertCommandFields>,
        /// 份额调整 kind（split）的语义字段与源端比对锚点；其余 kind 与旧版本
        /// 设备产出的载荷（该成员缺省）为 None（ADR-0106 决策 9 / issue #1053）。
        #[serde(default)]
        split: Option<SplitCommandFields>,
    },
    /// 删除交易（软删除）：实体 id 足够——重放端读行现状（kind 守卫、账户引用）
    /// 执行与本地删除同一协议。
    Delete { id: String },
}

impl TransactionCommand {
    /// 命令指向的实体 id（创建/修改随行携带，删除即目标 id）。
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            TransactionCommand::Create { id, .. } | TransactionCommand::Update { id, .. } => id,
            TransactionCommand::Delete { id } => id,
        }
    }
}

/// 协议面契约（#1089）：实体标签与 serde 信封 tag 同源（门 a 二源断言之锚），
/// 实体键派生是域自身知识；op 产出直呼协议面（环依赖由 crate 依赖图断开）。
impl SyncCommand for TransactionCommand {
    const ENTITY: &'static str = "transaction";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}
