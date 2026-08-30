//! 按代码即拉添加场外基金（issue #301 / ADR-0038 决策 1）：手动输入 6 位基金
//! 代码 → 东财详情拉取（名称 / 分类 / 最新单位净值 + 净值日期）→ 落标的字典
//! （类型 fund、市场恒 unknown、来源 manual）与现价缓存（净值即价格、币种人民
//! 币、带净值日期）。查无此码返回中文错误，不产生标的行。
//!
//! 编排与网络解耦：核心接缝 [`add_fund_by_code_with`] 接受注入的详情获取函数
//! （`&str → Result<FundDetail>`），测试与 BDD 以 stub 离线驱动（不依赖真实
//! 网络）；生产命令在锁外完成网络拉取后调 [`persist_fund_detail`] 落库
//! （见 `commands::investment::add_fund_by_code`）。

use rusqlite::Connection;

use crate::commands::sync::persist::{price_value_to_cents, upsert_market_price};
use crate::error::{AppError, Result};
use crate::models::{AddFundResult, FundDetail, InstrumentInput, InstrumentType};

use super::crud;

/// 场外基金标的的固定字典形态（ADR-0038 决策 1）：类型 fund、市场恒 unknown
/// （场外基金无交易所市场概念，纯字典键）、币种人民币（含 QDII 人民币份额）。
const FUND_MARKET: &str = "unknown";
const FUND_CURRENCY: &str = "CNY";

/// 基金代码是否可进净值同步通道：6 位纯数字（入口收口的安全前提之一——
/// 自建标的 UI 白名单不含 fund、AI 端点查无此码拒绝，fund 行只经按代码即拉
/// 通道产生，自编 6 位代码无产生通道，ADR-0038 决策 6）。按代码即拉入口的
/// 校验（[`validate_fund_code`]）与净值同步分区的「可拉取」判定同源本谓词。
pub(crate) fn is_syncable_fund_code(code: &str) -> bool {
    code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit())
}

/// 基金代码合法性校验：同 [`is_syncable_fund_code`]（按代码即拉入口形态）。
pub(crate) fn validate_fund_code(code: &str) -> Result<()> {
    if is_syncable_fund_code(code) {
        Ok(())
    } else {
        Err(AppError::Invalid("基金代码须为 6 位数字".into()))
    }
}

/// 拉取到的基金详情落库：建标的行（复用核心创建函数的（代码，类型）幂等
/// upsert，来源 manual，ADR-0036）+ 有净值时落现价缓存（净值即价格、
/// priced_at = 净值日期）。返回结果含 `price_written`（价格失效信号判定依据）。
pub(crate) fn persist_fund_detail(
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
