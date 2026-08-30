//! 投资域市场数据 CRUD（汇率 / 行情 / 标的字典，与持仓报告无关的字典写入）。
//!
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：本模块对备份域
//! 零感知，写入成功后的置脏/到期检查由调用方所在写入口闭包在提交点单点执行。

use rusqlite::Connection;

use super::predicates::INVESTED_EXISTS;
use crate::commands::search::text::{split_terms, term_matches_text};
use crate::db::query::query_all;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{
    ExchangeRate, ExchangeRateInput, Holding, Instrument, InstrumentInput, InstrumentListFilter,
    InstrumentListResult, MarketPrice, MarketPriceInput,
};

pub(crate) fn list_holdings(conn: &Connection) -> Result<Vec<Holding>> {
    query_all(
        conn,
        "SELECT id,account_id,instrument_id,quantity,cost_basis_cents,cost_currency_code, \
         latest_price_cents,latest_price_currency_code,market_value_cents,unrealized_pnl_cents,updated_at \
         FROM v_holdings ORDER BY account_id, instrument_id",
        [],
    )
}

pub(crate) fn list_exchange_rates(conn: &Connection) -> Result<Vec<ExchangeRate>> {
    query_all(
        conn,
        "SELECT id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id \
         FROM exchange_rates ORDER BY base_code, quote_code",
        [],
    )
}

pub(crate) fn create_exchange_rate(conn: &Connection, input: ExchangeRateInput) -> Result<String> {
    if input.rate <= 0.0 {
        return Err(AppError::Invalid("汇率必须大于 0".into()));
    }
    let id = new_uuid();
    let now = now_iso();
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
            rusqlite::params![input.base_code, input.quote_code],
            |r| r.get(0),
        )
        .ok();
    let id = existing_id.unwrap_or(id);
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) \
         ON CONFLICT(base_code, quote_code) DO UPDATE SET \
         rate=excluded.rate, priced_at=excluded.priced_at, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1, device_id=excluded.device_id",
        rusqlite::params![
            id,
            input.base_code,
            input.quote_code,
            input.rate,
            input.priced_at,
            input.source,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

pub(crate) fn list_market_prices(conn: &Connection) -> Result<Vec<MarketPrice>> {
    query_all(
        conn,
        "SELECT id,instrument_id,price_cents,currency_code,priced_at,nav_date,source,created_at,updated_at,version,device_id \
         FROM market_prices ORDER BY instrument_id, priced_at DESC",
        [],
    )
}

pub(crate) fn create_market_price(conn: &Connection, input: MarketPriceInput) -> Result<String> {
    if input.price_cents <= 0 {
        return Err(AppError::Invalid("价格必须大于 0".into()));
    }
    // 写入委托现价缓存单点 upsert（issue #291 收口：原就地 SQL 与 sync::persist
    // 两份同形 upsert 合并为一份）；手动落价无净值日期语义，nav_date 覆盖为 NULL
    // （防基金现价被手动更新后旧净值日期残留错配，与 sync::persist 同规则）。
    // source 透传入参（可空，发布 API 形状不变）；手动报价正经 manual_price 模块
    // （record_manual_price），本命令为已发布的独立写价通道（issue #291 前的半成品）。
    // 返回落库行实际 id（upsert 单点负责既有行复用/新建）。
    crate::commands::sync::persist::upsert_market_price(
        conn,
        &input.instrument_id,
        input.price_cents,
        &input.currency_code,
        &input.priced_at,
        None,
        input.source.as_deref(),
    )
}

/// 标的搜索的匹配目标：「代码 · 名称」label 等价文本（与投资表单标的下拉的
/// 选项 label 一致；无名称时退化为裸代码）。收口为具名函数，语义变更只改这里。
fn instrument_match_label(inst: &Instrument) -> String {
    match inst.name.as_deref().filter(|n| !n.is_empty()) {
        Some(name) => format!("{} · {}", inst.symbol, name),
        None => inst.symbol.clone(),
    }
}

pub(crate) fn list_instruments(
    conn: &Connection,
    filter: &InstrumentListFilter,
) -> Result<InstrumentListResult> {
    // 关键字过滤走统一模糊搜索语义（ADR-0027，复用全局搜索纯函数）：词条之间
    // AND，命中 = 原文连续子串 ∨ 拼音首字母子序列（大小写不敏感）。判定目标为
    // 「代码 · 名称」label 等价文本（instrument_match_label，与投资表单标的
    // 下拉的 label 一致）。子序列匹配无法下推 SQL，故有搜索词时取候选后
    // Rust 内存过滤再内存分页。
    let search_terms = filter
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(split_terms);

    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(market) = filter.market.as_deref().filter(|m| !m.is_empty()) {
        params.push(Box::new(market.to_string()));
        conditions.push(format!("i.market=?{}", params.len()));
    }
    // 标的类型过滤（issue #294）：同码异类型消歧（如基金 000001 vs 股票 000001）。
    if let Some(kind) = filter.kind {
        params.push(Box::new(kind.to_string()));
        conditions.push(format!("i.instrument_type=?{}", params.len()));
    }
    // 只看持仓：有当前持仓的标的，谓词单点见 predicates 模块（别名契约：i = instruments）。
    if filter.only_invested == Some(true) {
        conditions.push(INVESTED_EXISTS.to_string());
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let select_sql = |limit_clause: &str| {
        format!(
            "SELECT i.id,i.symbol,i.instrument_type,i.name,i.currency_code,i.market,i.created_at,i.updated_at,i.version,i.device_id,i.source,p.price_cents,\n         CASE WHEN {INVESTED_EXISTS} THEN 1 ELSE 0 END AS invested \
             FROM instruments i \
             LEFT JOIN market_prices p ON p.instrument_id = i.id \
             {where_clause} ORDER BY i.symbol{limit_clause}"
        )
    };

    let page = filter.page.unwrap_or(1).max(1);
    let page_size = filter.page_size.unwrap_or(50).clamp(1, 500);
    let offset = (page - 1) * page_size;

    let (total, items) = if let Some(terms) = &search_terms {
        // 语义匹配分支：全量候选后 Rust 过滤，total = 命中数，内存分页。
        let all: Vec<Instrument> = query_all(conn, &select_sql(""), params_ref.as_slice())?;
        let matched: Vec<Instrument> = all
            .into_iter()
            .filter(|inst| {
                terms
                    .iter()
                    .all(|t| term_matches_text(t, &instrument_match_label(inst)))
            })
            .collect();
        let total = matched.len() as i64;
        let items = matched.into_iter().skip(offset).take(page_size).collect();
        (total, items)
    } else {
        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM instruments i{where_clause}"),
            params_ref.as_slice(),
            |r| r.get(0),
        )?;
        let mut params = params;
        params.push(Box::new(page_size as i64));
        params.push(Box::new(offset as i64));
        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let items = query_all(
            conn,
            &select_sql(&format!(
                " LIMIT ?{} OFFSET ?{}",
                params.len() - 1,
                params.len()
            )),
            params_ref.as_slice(),
        )?;
        (total, items)
    };

    Ok(InstrumentListResult { items, total })
}

/// 核心创建函数（手动 IPC 命令与 AI HTTP 端点共用，ADR-0037）：新建行来源标
/// 'manual'（非同步即手动），（代码，类型）命中既有行则复用并只更新名称/市场，
/// 来源随行终身不变（issue #293 / ADR-0036 决策 2）。
pub(crate) fn create_instrument(conn: &Connection, input: InstrumentInput) -> Result<String> {
    if input.symbol.trim().is_empty() {
        return Err(AppError::Invalid("标的代码不能为空".into()));
    }
    let market = input.market.as_deref().unwrap_or("unknown");
    let existing_id: Option<(String, Option<String>, String)> = conn
        .query_row(
            "SELECT id, name, market FROM instruments WHERE symbol=?1 AND instrument_type=?2",
            rusqlite::params![input.symbol, input.kind],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    if let Some((existing_id, existing_name, existing_market)) = existing_id {
        let name_changed = input.name != existing_name;
        let market_changed = market != existing_market;
        if name_changed || market_changed {
            let now = now_iso();
            conn.execute(
                "UPDATE instruments SET name=?1, market=?2, updated_at=?3, version=version+1 WHERE id=?4",
                rusqlite::params![input.name, market, now, existing_id],
            )?;
        }
        return Ok(existing_id);
    }
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id,source) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'manual')",
        rusqlite::params![
            id,
            input.symbol,
            input.kind,
            input.name,
            input.currency_code,
            market,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}
