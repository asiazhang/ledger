//! 场外基金按代码取价编排（issue #301 / ADR-0038 决策 1；换源 ADR-0130 决策 2 /
//! issue #1568）：按 6 位基金代码取报价（权威名称 / 最新单位净值 + 净值日期），
//! 投影为行情接入接缝的统一载荷 [`Quote`]（ADR-0103 决策 2）；供「按代码即拉」
//! 添加基金、AI 查询/创建端点（#304 / ADR-0039）、名称随行刷新通道（#827）复用，
//! 即接缝的**查询半边**（按代码取行情，纯取数不落库）。
//!
//! 取数编排三臂（新浪为主源、证监会基金电子披露为权威兜底与判定源）：
//! 1. **新浪批量面**（`f_` 前缀，单只 = 一批一条，取数单元 [`super::sina_fund`]）：
//!    普通行直接给出名称与最新单位净值——在用基金与已终止普通基金都在面（实测
//!    清盘样本末点照常在），一次请求即答；
//! 2. **货基判定确认**（批量面为货基错位行——万份收益在单位净值位、产不出价格
//!    点——时经官方披露自报形态确认，[`super::csrc::confirm_money_fund_form`]）：
//!    确认即按恒定单位净值 1.0000 落恒定价并携带恒定价格信号（ADR-0126 决策 3；
//!    万份收益永不进价，#1342）；缺信号落第 3 臂；
//! 3. **证监会披露区间查询**（[`super::csrc::fetch_fund_nav_series`]，已终止基金
//!    存在性与最后一期净值的权威兜底面）：批量面未收录的代码在此改判存在性，
//!    最新披露记录给出名称与取值形态（货基自报形态 → 恒定价，普通形态 → 单位
//!    净值）。
//!
//! 「查无此码」的唯一结论来源是第 3 臂的可信空报文（结构完好且记录为空）——
//! 批量面的空值语句只说明「此面未收录」，不宣布不存在；披露源不可信（Err）按
//! fail-closed 上抛，不降级为查无此码。基金分类在替代源无来源：恒缺省（契约
//! 投影为空串，ADR-0130 决策 8，不用名称关键词推导）。价格来源标记随取数产物
//! 携带（[`Quote::price_source`]，ADR-0130 决策 7：批量面记 `sina`、披露臂记
//! `csrc`）。
//!
//! 解析与行形态判别归各取数单元（`sina_fund` / `csrc`，fixture 单测见各自测试
//! 文件）；本模块只做臂间编排与 [`Quote`] 投影。网络请求复用行情 HTTP 层的
//! 主机池 / 重试 / 限流。

use serde::Deserialize;

use ledger_infra::error::{AppError, Result};
use ledger_investment::Quote;
use ledger_investment::prices::{CSRC_PRICE_SOURCE, SINA_PRICE_SOURCE, price_value_to_cents};

use super::csrc::{
    CSRC_HOSTS, confirm_money_fund_form_from, disclosure_window_dates, fetch_fund_nav_series_from,
};
use super::http::{Pacer, build_client};
use super::sina_fund::{SINA_FUND_BATCH_HOSTS, SinaFundNavForm, fetch_sina_fund_nav_rows};

/// 数值字段兼容数字与数字字符串两种 wire 形态；非数值（含 null）按缺省处理。
/// 历史净值接口（fund_nav）与新浪/披露取数单元共用。
pub(super) fn deserialize_flexible_f64<'de, D>(d: D) -> std::result::Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(d)?;
    Ok(match value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    })
}

/// 字符串字段兼容任意 wire 形态且**不使报文失败**（基金类型码等判定信号）：字符串去首尾空白；其余形态（数字、null 等）归为
/// 缺省——信号缺席的代价只是退回修复前口径，不得让整页解析失败中断同步。
pub(super) fn deserialize_flexible_string<'de, D>(
    d: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(s) => {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        }
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

/// 按 6 位代码取基金报价（三臂编排，见模块文档）：新浪批量面 → 货基判定确认 →
/// 证监会披露区间兜底。批量面取数失败（被拦截 / 不可信形状）与披露源不可信
/// 一律上抛，不静默降级；两源皆未收录才返回查无此码的码化错误。
pub(super) async fn fetch_fund_quote(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    code: &str,
) -> Result<Quote> {
    fetch_fund_quote_from(client, pacer, code, SINA_FUND_BATCH_HOSTS, CSRC_HOSTS).await
}

/// 同 [`fetch_fund_quote`]，两个源的主机池都可注入（本地 HTTP 服务测试编排三臂
/// 与请求形态）。
pub(super) async fn fetch_fund_quote_from(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    code: &str,
    batch_hosts: &[&str],
    csrc_hosts: &[&str],
) -> Result<Quote> {
    tracing::debug!(code, "基金行情查询");
    // 臂 1：新浪批量面（单只 = 一批一条）。普通行一次请求即答。
    let rows = fetch_sina_fund_nav_rows(client, pacer, batch_hosts, &[code.to_string()]).await?;
    if let Some(row) = rows.into_iter().find(|row| row.code == code) {
        if let SinaFundNavForm::UnitNav { unit_nav, .. } = row.form {
            return Ok(unit_nav_quote(
                code,
                &row.name,
                &row.nav_date,
                unit_nav,
                SINA_PRICE_SOURCE,
            ));
        }
        // 货基错位行：万份收益在单位净值位，不产出价格点——判定确认后才定价。
        match confirm_money_fund_form_from(client, pacer, code, csrc_hosts).await {
            Ok(true) => return Ok(constant_price_quote(code, &row.name, &row.nav_date)),
            // 缺信号不是反证，但批量面给不出可采信价格——落权威兜底臂取披露记录。
            Ok(false) => {}
            Err(error) => return Err(error),
        }
    }
    // 臂 3：证监会披露区间查询（存在性 + 名称 + 最后一期净值的权威兜底）。
    // 窗口拉宽覆盖已终止基金的存量披露（披露止于终止日，终止越深记录越靠前）。
    let (start, end) = disclosure_window_dates();
    let records = fetch_fund_nav_series_from(client, pacer, code, &start, &end, csrc_hosts).await?;
    let latest = records
        .into_iter()
        .max_by(|a, b| a.valuation_date.cmp(&b.valuation_date));
    let Some(latest) = latest else {
        // 可信空报文是「查无此码」的唯一结论来源（解析层保证空序列可信）。
        return Err(fund_not_found(code));
    };
    if latest.is_money_fund_form() {
        return Ok(constant_price_quote(
            code,
            &latest.name,
            &latest.valuation_date,
        ));
    }
    match latest.unit_nav {
        Some(nav) => Ok(unit_nav_quote(
            code,
            &latest.name,
            &latest.valuation_date,
            nav,
            // 价格自官方披露取得（兜底臂，ADR-0130 决策 7）。
            CSRC_PRICE_SOURCE,
        )),
        // 普通形态记录缺单位净值（仅累计等字段有值）：名称可用、无价可落。
        None => Ok(name_only_quote(code, &latest.name)),
    }
}

/// 普通净值行报价投影（新浪批量面臂与披露兜底臂共用，来源由调用臂带入）：
/// 名称 + 最新单位净值（净值即价格、万分之一元刻度），价格日期与净值日期同为
/// 净值日期（现价的行情日期就是净值本身对应的日期）。场外通道成员（基金分类、
/// 恒定价格信号、精确市场、类型提示）按通道形态缺省。
fn unit_nav_quote(code: &str, name: &str, nav_date: &str, nav: f64, source: &'static str) -> Quote {
    Quote {
        code: code.to_string(),
        name: name.trim().to_string(),
        price_cents: Some(price_value_to_cents(nav)),
        price_date: Some(nav_date.to_string()),
        market: None,
        kind_hint: None,
        fund_class: None,
        nav_date: Some(nav_date.to_string()),
        constant_unit_price_cents: None,
        price_source: source,
    }
}

/// 货基（官方披露自报形态确认）报价投影：现价 = 恒定单位净值 1.0000（万份收益
/// 永不进价，ADR-0126 / ADR-0130 决策 6），恒定价格信号随载荷带回落库半边打标
///（建档一次确认，单向幂等）；价格日期与净值日期同为最新披露（收益）日期。
/// 来源记证监会披露——恒定价格的**确认源**是官方自报形态。
fn constant_price_quote(code: &str, name: &str, date: &str) -> Quote {
    let cents = price_value_to_cents(super::fund_nav::MONEY_FUND_UNIT_NAV);
    Quote {
        code: code.to_string(),
        name: name.trim().to_string(),
        price_cents: Some(cents),
        price_date: Some(date.to_string()),
        market: None,
        kind_hint: None,
        fund_class: None,
        nav_date: Some(date.to_string()),
        constant_unit_price_cents: Some(cents),
        price_source: CSRC_PRICE_SOURCE,
    }
}

/// 名称可用、无价可落的报价投影（披露记录缺单位净值、批量面缺信号等形态）：
/// 未取到净值不落现价，标的行仍以权威名称建成。
fn name_only_quote(code: &str, name: &str) -> Quote {
    Quote {
        code: code.to_string(),
        name: name.trim().to_string(),
        price_cents: None,
        price_date: None,
        market: None,
        kind_hint: None,
        fund_class: None,
        nav_date: None,
        constant_unit_price_cents: None,
        price_source: CSRC_PRICE_SOURCE,
    }
}

/// 「查无此码」码化错误（Invalid → 400）：批量面未收录且官方披露可信空——两源
/// 皆未命中时的唯一出口。
fn fund_not_found(code: &str) -> AppError {
    AppError::codedp(
        "sync.fund-not-found",
        format!("查无基金代码 {code}，请核对后重试"),
        &[code],
    )
}

/// 生产拉取入口：构建客户端与限流器后执行单次查询（不经数据库连接，
/// 供两壳在连接锁外完成网络往返，避免长限流重试阻塞其它命令）。async 形态
///（ADR-0125 决策 5/7，issue #1413）：网络等待以 `await` 表达，在异步上下文
/// 内直接可调，#1411 的过渡同步桥已随接缝 async 化拆除。
pub async fn fetch_fund_quote_production(code: &str) -> Result<Quote> {
    let client = build_client()?;
    let mut pacer = Pacer::default();
    fetch_fund_quote(&client, &mut pacer, code).await
}
