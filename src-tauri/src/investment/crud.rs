//! 投资域市场数据 CRUD（汇率 / 行情 / 标的字典，与持仓报告无关的字典写入）。
//!
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：本模块对备份域
//! 零感知，写入成功后的置脏/到期检查由调用方所在写入口闭包在提交点单点执行。

use rusqlite::Connection;

use super::predicates::INVESTED_EXISTS;
use super::prices::upsert_market_price;
use crate::db::query::query_all;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{
    ExchangeRate, ExchangeRateInput, Holding, Instrument, InstrumentInput, InstrumentListFilter,
    InstrumentListResult, InstrumentType, MarketPrice, MarketPriceInput,
};
use crate::transaction::search_text::{split_terms, term_matches_text};

pub fn list_holdings(conn: &Connection) -> Result<Vec<Holding>> {
    query_all(
        conn,
        "SELECT id,account_id,instrument_id,quantity,cost_basis_cents,cost_currency_code, \
         latest_price_cents,latest_price_currency_code,latest_nav_date,market_value_cents,unrealized_pnl_cents,updated_at \
         FROM v_holdings ORDER BY account_id, instrument_id",
        [],
    )
}

pub fn list_exchange_rates(conn: &Connection) -> Result<Vec<ExchangeRate>> {
    query_all(
        conn,
        "SELECT id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id \
         FROM exchange_rates ORDER BY base_code, quote_code",
        [],
    )
}

pub fn create_exchange_rate(conn: &Connection, input: ExchangeRateInput) -> Result<String> {
    if input.rate <= 0.0 {
        return Err(AppError::coded("fx.rate-positive", "汇率必须大于 0"));
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

pub fn list_market_prices(conn: &Connection) -> Result<Vec<MarketPrice>> {
    query_all(
        conn,
        "SELECT id,instrument_id,price_cents,currency_code,priced_at,nav_date,source,created_at,updated_at,version,device_id \
         FROM market_prices ORDER BY instrument_id, priced_at DESC",
        [],
    )
}

pub fn create_market_price(conn: &Connection, input: MarketPriceInput) -> Result<String> {
    if input.price_cents <= 0 {
        return Err(AppError::coded(
            "instrument.price-positive",
            "价格必须大于 0",
        ));
    }
    // 写入委托现价缓存单点 upsert（issue #291 收口：原就地 SQL 与同步通道
    // 两份同形 upsert 合并为一份）；手动落价无净值日期语义，nav_date 覆盖为 NULL
    // （防基金现价被手动更新后旧净值日期残留错配，与同步通道同规则）。
    // source 透传入参（可空，发布 API 形状不变）；手动报价正经 manual_price 模块
    // （record_manual_price），本命令为已发布的独立写价通道（issue #291 前的半成品）。
    // 返回落库行实际 id（upsert 单点负责既有行复用/新建）。
    upsert_market_price(
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

pub fn list_instruments(
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

/// 自建标的物理删除（issue #292 / ADR-0036 决策 5）：守卫前置检查——仅来源为
/// 手动且无任何 buy/sell 流水引用（security_transactions 无行）的自建标的可删；
/// 有引用拒删（交易行与明细归属用户记账事实，不随字典清理）、同步来源标的拒删
/// （填错由全量同步修正）。不引入软删——标的字典查询面不被污染。现价缓存与
/// 价格历史随外键 CASCADE 一并消失；持仓批次表虽是 RESTRICT，但批次行的
/// buy_transaction_id 为指向 security_transactions 的 NOT NULL 外键——批次存在
/// 必有买入明细行，故守卫的流水 COUNT 已覆盖批次（无流水 ⟺ 无批次），
/// DELETE 不会撞到 RESTRICT 外键错误。
pub fn delete_instrument(conn: &Connection, id: &str) -> Result<()> {
    let source: Option<String> = conn
        .query_row(
            "SELECT source FROM instruments WHERE id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .ok();
    let source = source.ok_or_else(|| {
        AppError::codedp_not_found("instrument.not-found", format!("标的 {id} 不存在"), &[id])
    })?;
    if source != "manual" {
        return Err(AppError::coded(
            "instrument.sync-delete-forbidden",
            "同步来源标的不支持删除：股票字典由「全量同步」维护，填错可重新同步修正",
        ));
    }
    let trade_refs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM security_transactions WHERE instrument_id=?1",
        rusqlite::params![id],
        |r| r.get(0),
    )?;
    if trade_refs > 0 {
        return Err(AppError::coded(
            "instrument.traded-delete-forbidden",
            "该标的已有买卖流水，无法删除：可先删除相关交易后再试",
        ));
    }
    conn.execute("DELETE FROM instruments WHERE id=?1", rusqlite::params![id])?;
    Ok(())
}

/// 核心创建函数（手动 IPC 命令与 AI HTTP 端点共用，ADR-0037）：新建行来源标
/// 'manual'（非同步即手动），（代码，类型）命中既有行则复用并只更新名称/市场，
/// 来源随行终身不变（issue #293 / ADR-0036 决策 2）。
pub fn create_instrument(conn: &Connection, input: InstrumentInput) -> Result<String> {
    if input.symbol.trim().is_empty() {
        return Err(AppError::coded(
            "instrument.symbol-required",
            "标的代码不能为空",
        ));
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

/// 手动创建入口守卫（ADR-0036 决策 3）：类型白名单收窄为债券/ETF/其他三类——
/// 股票字典归全量同步修、基金唯一创建入口归按代码即拉（issue #301 / ADR-0038），
/// 白名单让手动字典与两条自动通道永不相交；名称必填（自建标的主身份是名称）。
/// 守卫属 UI 入口政策，核心创建函数 [`create_instrument`] 保持通用：AI HTTP
/// 创建端点（ADR-0037）五类全开、名称可选，不经本守卫。同一接缝供 IPC 命令
/// 与 BDD 步骤复用。
pub fn create_instrument_manual(conn: &Connection, input: InstrumentInput) -> Result<String> {
    match input.kind {
        InstrumentType::Bond | InstrumentType::Etf | InstrumentType::Other => {}
        InstrumentType::Stock => {
            return Err(AppError::coded(
                "instrument.stock-manual-forbidden",
                "股票类标的不支持手动创建：股票字典由「全量同步」从东方财富维护",
            ));
        }
        InstrumentType::Fund => {
            return Err(AppError::coded(
                "instrument.fund-manual-forbidden",
                "基金类标的不支持手动创建：请用「添加基金」输入 6 位代码自动回填",
            ));
        }
    }
    if input.name.as_deref().is_none_or(|n| n.trim().is_empty()) {
        return Err(AppError::coded(
            "instrument.name-required",
            "标的名称不能为空",
        ));
    }
    create_instrument(conn, input)
}
