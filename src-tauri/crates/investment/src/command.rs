//! 投资域同步命令（issue #861 / ADR-0091）：标的字典、汇率与用户侧价格写入的
//! op 载荷形态、产出单点与重放分派。
//!
//! - **载荷形态**（serde：`action` 判别；作为 DomainCommand 信封的 payload 内嵌）：
//!   - [`InstrumentCommand`]：建档携带实体 id + 语义行（重放端不得重新生成 id）；
//!     幂等复用改名携带自然键与落定名称/市场；删除只需实体 id。建档 id 各端独立
//!     生成（并发建档各自成行、自然键 upsert 收敛业务字段），故改名/删除的 LWW
//!     裁决以携带 id 为域、重放执行以自然键定位（身份与定位分离，见各函数注释）。
//!   - [`ExchangeRateCommand`]：upsert 携带行身份（货币对自然键）与语义行，
//!     裁决域 = 货币对。
//!   - [`PriceCommand`]：现价录入裁决域 = 标的的现价行（`market_prices` 每标的
//!     一行）；手动报价裁决域 = 标的 × ISO 周的周采样行（同周后写覆盖先写，
//!     裁决键与落库冲突键同粒度，跨周报价互不压制、全部沉淀）。**只增不改**。
//! - **行情数据不进 op**：东财外拉的现价/净值/权威名称刷新（`sync` 域、按代码
//!   即拉/创建增强的价格落库）是外部事实，各端自行拉取——「同步 ≠ 行情同步」
//!   （CONTEXT-sync Transport 词条）；本命令面只承载用户产生的数据变化。
//! - **产出单点**（`record_*`）：投资域各写编排入口（`crud` / `manual_price`）
//!   成功后调用，op 随写事务提交/回滚，写失败不残留 op。
//! - **重放执行**（`replay_*`）：与本地写同一执行协议（守卫原样生效，依赖缺失
//!   以码化错误上抛 → 引擎挂起待裁决，依赖方补齐后重投递自愈），不产出 op。

use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::InstrumentType;
use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

/// 标的命令行载荷（语义字段；簿记戳不随行携带，市场为解析后的落定值）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstrumentCommandRow {
    pub symbol: String,
    #[serde(rename = "type")]
    pub kind: InstrumentType,
    pub name: Option<String>,
    pub currency_code: String,
    pub market: String,
}

/// 标的字典同步命令。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum InstrumentCommand {
    /// 建档：实体 id 与语义行随命令携带（新建行用；重放端命中自然键复用既有行）。
    Create {
        id: String,
        row: InstrumentCommandRow,
    },
    /// 幂等复用改名（随用随修/AI 复用）：自然键定位、落定名称与市场随行。
    Update {
        id: String,
        symbol: String,
        kind: InstrumentType,
        name: Option<String>,
        market: String,
    },
    /// 删除（仅自建标的可删：守卫在重放端原样生效）。
    Delete { id: String },
}

impl InstrumentCommand {
    /// 命令指向的实体键（LWW 裁决域 = 单个标的行身份）。实体标签不在此返回——
    /// 由同步域重放注册表单源组装（ADR-0101 勘误 3）。
    pub(crate) fn subject(&self) -> Option<Cow<'_, str>> {
        match self {
            InstrumentCommand::Create { id, .. }
            | InstrumentCommand::Update { id, .. }
            | InstrumentCommand::Delete { id } => Some(Cow::Borrowed(id.as_str())),
        }
    }
}

/// 协议面契约（#1089）：实体标签与 serde 信封 tag 同源（门 a 二源断言之锚），
/// 实体键派生是域自身知识；op 产出直呼协议面（环依赖由 crate 依赖图断开）。
impl SyncCommand for InstrumentCommand {
    const ENTITY: &'static str = "instrument";

    fn subject(&self) -> Option<Cow<'_, str>> {
        // 同名转发：方法解析固有优先，落在上方域自身实现（非本 trait 方法）。
        InstrumentCommand::subject(self)
    }
}

/// 汇率同步命令（货币对为行身份：`exchange_rates` 每对一行、upsert 语义）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ExchangeRateCommand {
    Upsert {
        id: String,
        base_code: String,
        quote_code: String,
        rate: f64,
        priced_at: String,
        source: Option<String>,
    },
}

impl ExchangeRateCommand {
    /// 命令指向的实体键（LWW 裁决域 = 货币对自然键；行 id 各端可异，不作裁决域）。
    /// 键由命令字段确定性派生（`sync_ops.entity_id` 列同源；随行的建档 id 与
    /// 裁决无关——同货币对并发录入取序末者）。实体标签不在此返回——由同步域
    /// 重放注册表单源组装（ADR-0101 勘误 3）。
    pub(crate) fn subject(&self) -> Option<Cow<'_, str>> {
        match self {
            ExchangeRateCommand::Upsert {
                base_code,
                quote_code,
                ..
            } => Some(Cow::Owned(format!("{base_code}->{quote_code}"))),
        }
    }
}

/// 协议面契约（#1089）：实体标签与 serde 信封 tag 同源（门 a 二源断言之锚），
/// 实体键派生是域自身知识（货币对自然键）；op 产出直呼协议面（环依赖由
/// crate 依赖图断开）。
impl SyncCommand for ExchangeRateCommand {
    const ENTITY: &'static str = "exchange_rate";

    fn subject(&self) -> Option<Cow<'_, str>> {
        // 同名转发：方法解析固有优先，落在上方域自身实现（非本 trait 方法）。
        ExchangeRateCommand::subject(self)
    }
}

/// 用户侧价格同步命令：现价录入与手动报价（东财行情通道不产出本命令）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PriceCommand {
    /// 现价录入（独立写价通道）：现价缓存 upsert，裁决域 = 标的的现价行。
    MarketPrice {
        instrument_id: String,
        price_cents: i64,
        currency_code: String,
        priced_at: String,
        source: Option<String>,
    },
    /// 手动报价：一条通道两落点（周采样历史 + 现价映像），落库判定在执行端
    /// 按同一规则进行（最新点映像规则依赖本地历史状态，同序重放 ⇒ 同一判定）。
    ManualPrice {
        instrument_id: String,
        date: String,
        price_cents: i64,
    },
}

impl PriceCommand {
    /// 命令指向的实体键（LWW 裁决域与落库冲突键同粒度：现价行按标的、周采样行
    /// 按标的 × ISO 周——跨周报价不互压，同周报价取序末者）。实体标签不在此
    /// 返回——由同步域重放注册表单源组装的 `price`（与 serde tag 同源，ADR-0101
    /// 勘误 2 归一，复活 price 类 op 的 LWW）。
    pub(crate) fn subject(&self) -> Option<Cow<'_, str>> {
        match self {
            PriceCommand::MarketPrice { instrument_id, .. } => {
                Some(Cow::Borrowed(instrument_id.as_str()))
            }
            PriceCommand::ManualPrice {
                instrument_id,
                date,
                ..
            } => Some(Cow::Owned(format!(
                "{instrument_id}|{}",
                week_start_of(date)
            ))),
        }
    }
}

/// 协议面契约（#1089）：实体标签与 serde 信封 tag 同源（门 a 二源断言之锚），
/// 实体键派生是域自身知识（现价行按标的、周采样行按标的 × ISO 周，与落库
/// 冲突键同粒度）；op 产出直呼协议面（环依赖由 crate 依赖图断开）。
impl SyncCommand for PriceCommand {
    const ENTITY: &'static str = "price";

    fn subject(&self) -> Option<Cow<'_, str>> {
        // 同名转发：方法解析固有优先，落在上方域自身实现（非本 trait 方法）。
        PriceCommand::subject(self)
    }
}

/// ISO 周周一派生（与 `price_history.week_start` 生成列
/// `date(trade_date,'-6 days','weekday 1')` 同式的 Rust 形态）：裁决键必须与
/// 周采样落库冲突键同粒度，两端对同一报价才收敛到同一裁决域。解析失败回退
/// 原串（伪造载荷由重放执行的日期校验挂起承接，不影响裁决键确定性）。
fn week_start_of(date: &str) -> String {
    use chrono::Datelike;
    chrono::NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d")
        .map(|d| {
            (d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64))
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|_| date.to_string())
}

/// op 产出接缝（投资域集中单点）：标的字典写成功后追加一条 op 进本机 OpLog。
///
/// 仅投资域写编排入口（`investment::crud`）调用；随编排事务提交/回滚。
pub(crate) fn record_instrument(conn: &Connection, command: InstrumentCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

/// op 产出接缝：汇率录入成功后追加。
pub(crate) fn record_exchange_rate(conn: &Connection, command: ExchangeRateCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

/// op 产出接缝：用户侧价格写入（现价录入 / 手动报价）成功后追加。
pub(crate) fn record_price(conn: &Connection, command: PriceCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

/// 重放执行（同步引擎分派接缝）：按动作转发到与本地写同一执行协议。
pub fn replay_instrument_command(conn: &Connection, command: &InstrumentCommand) -> Result<()> {
    match command {
        InstrumentCommand::Create { id, row } => {
            super::crud::write_instrument(conn, id, row)?;
            Ok(())
        }
        InstrumentCommand::Update {
            symbol,
            kind,
            name,
            market,
            ..
        } => super::crud::write_instrument_update(conn, symbol, *kind, name.as_deref(), market),
        InstrumentCommand::Delete { id } => super::crud::write_delete_instrument(conn, id),
    }
}

/// 重放执行：汇率 upsert（与本地录入同一写协议，未命中以源端行 id 新建）。
pub fn replay_exchange_rate_command(
    conn: &Connection,
    command: &ExchangeRateCommand,
) -> Result<()> {
    match command {
        ExchangeRateCommand::Upsert {
            id,
            base_code,
            quote_code,
            rate,
            priced_at,
            source,
        } => {
            super::crud::write_exchange_rate(
                conn,
                id,
                base_code,
                quote_code,
                *rate,
                priced_at,
                source.as_deref(),
            )?;
            Ok(())
        }
    }
}

/// 重放执行：用户侧价格写入（与本地同一写协议；现价行未命中以落库单点新建）。
pub fn replay_price_command(conn: &Connection, command: &PriceCommand) -> Result<()> {
    match command {
        PriceCommand::MarketPrice {
            instrument_id,
            price_cents,
            currency_code,
            priced_at,
            source,
        } => {
            super::prices::upsert_market_price(
                conn,
                &super::prices::MarketPriceWrite {
                    instrument_id,
                    price_cents: *price_cents,
                    currency_code,
                    priced_at,
                    nav_date: None,
                    source: source.as_deref(),
                },
            )?;
            Ok(())
        }
        PriceCommand::ManualPrice {
            instrument_id,
            date,
            price_cents,
        } => super::manual_price::write_manual_price(
            conn,
            &super::model::ManualPriceInput {
                instrument_id: instrument_id.clone(),
                date: date.clone(),
                price_cents: *price_cents,
            },
        )
        .map(|_| ()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 周键派生与 SQLite 生成列同式：周一为首日（`weekday 1` 语义）。
    #[test]
    fn week_start_matches_sqlite_expression() {
        assert_eq!(week_start_of("2026-01-12"), "2026-01-12"); // 周一
        assert_eq!(week_start_of("2026-01-18"), "2026-01-12"); // 周日归本周一
        assert_eq!(week_start_of("2026-01-11"), "2026-01-05"); // 前一周日
    }
}
