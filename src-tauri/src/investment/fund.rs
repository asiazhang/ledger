//! 场外基金的东财详情落库接缝：按代码即拉添加（issue #301 / ADR-0038 决策 1）
//! 与 AI 创建端点 fund 增强（issue #304 / ADR-0039 决策 3）共用同一套字典形态——
//! 手动输入 / AI 提交 6 位基金代码 → 东财详情拉取（名称 / 分类 / 最新单位净值 +
//! 净值日期）→ 落标的字典（类型 fund、市场恒 unknown、来源 manual）与现价缓存
//! （净值即价格、币种人民币、带净值日期）。查无此码返回中文错误，不产生标的行。
//!
//! 编排与网络解耦：核心接缝 [`add_fund_by_code_with`] 接受注入的详情获取函数
//! （`&str → Result<FundDetail>`），测试与 BDD 以 stub 离线驱动（不依赖真实
//! 网络）；生产命令在锁外完成网络拉取后调 [`persist_fund_detail`] 落库
//! （见 `commands::investment` 壳的 `add_fund_by_code` 命令）。

use rusqlite::Connection;

use super::crud;
use super::prices::{EASTMONEY_PRICE_SOURCE, price_value_to_cents, upsert_market_price};
use crate::error::{AppError, Result};
use crate::models::{AddFundResult, FundDetail, InstrumentInput, InstrumentType};

/// 场外基金标的的固定字典形态（ADR-0038 决策 1）：类型 fund、市场恒 unknown
/// （场外基金无交易所市场概念，纯字典键）、币种人民币（含 QDII 人民币份额）。
const FUND_MARKET: &str = "unknown";
const FUND_CURRENCY: &str = "CNY";

/// 6 位纯数字判定（入口收口的安全前提，ADR-0038 决策 6 / ADR-0039 决策 3）：
/// 消费三方——按代码即拉的入口校验（[`validate_fund_code`]）、AI 端点 fund
/// 增强/查询的触发前提、净值同步分区的「可拉取」判定；名称充代码的 fund 行
///（源数据无代码）非 6 位，不触发东财校验、不进净值通道，自编 6 位代码无产生通道。
pub fn is_six_digit_code(code: &str) -> bool {
    code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit())
}

/// 基金代码合法性校验：同 [`is_six_digit_code`]（按代码即拉入口形态）。
pub fn validate_fund_code(code: &str) -> Result<()> {
    if is_six_digit_code(code) {
        Ok(())
    } else {
        Err(AppError::coded(
            "fund.code-invalid",
            "基金代码须为 6 位数字",
        ))
    }
}

/// 创建端点 fund 增强的落库结果（issue #304 / ADR-0039 决策 3）：标的 id +
/// 是否落现价（价格失效信号广播判定，语义同 `AddFundResult.price_written`）。
pub struct FundCreateOutcome {
    pub instrument_id: String,
    pub price_written: bool,
}

/// AI 创建端点 fund 增强的降级落库（issue #304 / ADR-0039 决策 3）：东财网络
/// 不可达等临时故障时，以 AI 提供的名称 + 真实代码建行（不阻塞导入，名称误差
/// 留待人工编辑）。字典形态与按代码即拉一致（类型 fund、市场 unknown、币种
/// 人民币）；既有行直接复用、名称不动——降级重放不得用 AI 名称覆盖已回填的
/// 东财权威名称。
pub fn create_fund_degraded(
    conn: &Connection,
    symbol: &str,
    ai_name: Option<String>,
) -> Result<FundCreateOutcome> {
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1 AND instrument_type='fund'",
            rusqlite::params![symbol],
            |r| r.get(0),
        )
        .ok();
    if let Some(instrument_id) = existing_id {
        return Ok(FundCreateOutcome {
            instrument_id,
            price_written: false,
        });
    }
    let instrument_id = crud::create_instrument(
        conn,
        InstrumentInput {
            symbol: symbol.to_string(),
            kind: InstrumentType::Fund,
            name: ai_name,
            currency_code: FUND_CURRENCY.to_string(),
            market: Some(FUND_MARKET.to_string()),
        },
    )?;
    Ok(FundCreateOutcome {
        instrument_id,
        price_written: false,
    })
}

/// 拉取到的基金详情落库：建标的行（复用核心创建函数的（代码，类型）幂等
/// upsert，来源 manual，ADR-0036）+ 有净值时落现价缓存（净值即价格、
/// priced_at = 净值日期）。返回结果含 `price_written`（价格失效信号判定依据）。
pub fn persist_fund_detail(
    conn: &Connection,
    code: &str,
    detail: &FundDetail,
) -> Result<AddFundResult> {
    let instrument_id = crud::create_instrument(
        conn,
        InstrumentInput {
            symbol: code.to_string(),
            kind: InstrumentType::Fund,
            name: Some(detail.name.clone()),
            currency_code: FUND_CURRENCY.to_string(),
            market: Some(FUND_MARKET.to_string()),
        },
    )?;
    let nav = detail.nav.as_ref();
    if let Some(nav) = nav {
        // 现价 = 最新公布单位净值（万分之一元，ADR-0038 决策 3）；priced_at 与
        // nav_date 同为净值日期——现价的行情日期就是净值本身对应的日期。覆盖
        // 不比较新旧净值日期：水位比较归净值同步通道（#303 以 nav_date 为增量
        // 水位），本通道语义 = 东财当前最新值整体回放。
        upsert_market_price(
            conn,
            &instrument_id,
            price_value_to_cents(nav.nav),
            FUND_CURRENCY,
            &nav.nav_date,
            Some(nav.nav_date.as_str()),
            Some(EASTMONEY_PRICE_SOURCE),
        )?;
    }
    Ok(AddFundResult {
        instrument_id,
        symbol: code.to_string(),
        name: detail.name.clone(),
        fund_class: detail.fund_class.clone(),
        nav_cents: nav.map(|n| price_value_to_cents(n.nav)),
        nav_date: nav.map(|n| n.nav_date.clone()),
        price_written: nav.is_some(),
    })
}

/// 按代码即拉核心接缝：校验代码 → 注入的获取函数拉详情 → 落库。
/// 获取函数按请求代码返回 [`FundDetail`]，查无此码以 `AppError::Invalid`
/// （中文错误）上抛；本函数不触碰网络，测试以 stub 驱动（先例：
/// `incremental::do_incremental_sync_with`）。
pub fn add_fund_by_code_with<F>(
    conn: &Connection,
    code: &str,
    fetch: &mut F,
) -> Result<AddFundResult>
where
    F: FnMut(&str) -> Result<FundDetail>,
{
    validate_fund_code(code)?;
    let detail = fetch(code)?;
    persist_fund_detail(conn, code, &detail)
}
