use rusqlite::Connection;

use super::command::{
    ExchangeRateCommand, InstrumentCommand, InstrumentCommandRow, PriceCommand,
    record_exchange_rate, record_instrument, record_price,
};
use super::fund::{FUND_MARKET, reject_non_unknown_fund_market};
use super::model::{
    Holding, Instrument, InstrumentInput, InstrumentListFilter, InstrumentListResult,
    InstrumentType, MANUAL_SOURCE, MarketPrice, MarketPriceInput,
};
use super::predicates::INVESTED_EXISTS;
use super::prices::{MarketPriceWrite, upsert_market_price};
use ledger_currencies::{ExchangeRate, ExchangeRateInput};
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;
use ledger_transaction::amount;
use ledger_transaction::shared::search_text::{split_terms, term_matches_text};

struct HoldingRow {
    holding: Holding,
    account_currency: String,
}

impl FromRow for HoldingRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(HoldingRow {
            holding: Holding::from_row(row)?,
            account_currency: row.get(12)?,
        })
    }
}

pub fn list_holdings(conn: &Connection) -> Result<Vec<Holding>> {
    let rows: Vec<HoldingRow> = query_all(
        conn,
        "SELECT v.id,v.account_id,v.instrument_id,v.quantity,v.cost_basis_cents,v.cost_currency_code, \
         v.latest_price_cents,v.latest_price_currency_code,v.latest_nav_date,v.market_value_cents,v.unrealized_pnl_cents,v.updated_at, \
         a.currency_code \
         FROM v_holdings v \
         JOIN accounts a ON a.id = v.account_id \
         ORDER BY v.account_id, v.instrument_id",
        [],
    )?;
    let mut holdings = Vec::with_capacity(rows.len());
    for row in rows {
        let mut holding = row.holding;
        holding.native_market_value_cents = match holding.market_value_cents {
            Some(cents) => {
                amount::try_convert_to_native_current(conn, cents, &row.account_currency)?
            }
            None => None,
        };
        holding.native_unrealized_pnl_cents = match holding.unrealized_pnl_cents {
            Some(cents) => {
                amount::try_convert_to_native_current(conn, cents, &row.account_currency)?
            }
            None => None,
        };
        holdings.push(holding);
    }
    Ok(holdings)
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
    ensure_transaction(conn, || {
        if input.rate <= 0.0 {
            return Err(AppError::coded("fx.rate-positive", "汇率必须大于 0"));
        }
        let id = write_exchange_rate(
            conn,
            &new_uuid(),
            &input.base_code,
            &input.quote_code,
            input.rate,
            &input.priced_at,
            input.source.as_deref(),
        )?;
        record_exchange_rate(
            conn,
            ExchangeRateCommand::Upsert {
                id: id.clone(),
                base_code: input.base_code,
                quote_code: input.quote_code,
                rate: input.rate,
                priced_at: input.priced_at,
                source: input.source,
            },
        )?;
        Ok(id)
    })
}

pub(crate) fn write_exchange_rate(
    conn: &Connection,
    insert_id: &str,
    base_code: &str,
    quote_code: &str,
    rate: f64,
    priced_at: &str,
    source: Option<&str>,
) -> Result<String> {
    let now = now_iso();
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
            rusqlite::params![base_code, quote_code],
            |r| r.get(0),
        )
        .ok();
    let id = existing_id.unwrap_or_else(|| insert_id.to_string());
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) \
         ON CONFLICT(base_code, quote_code) DO UPDATE SET \
         rate=excluded.rate, priced_at=excluded.priced_at, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1, device_id=excluded.device_id",
        rusqlite::params![
            id,
            base_code,
            quote_code,
            rate,
            priced_at,
            source,
            now,
            1,
            device_id(conn)?
        ],
    )?;
    Ok(id)
}

pub fn upsert_auto_exchange_rate(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
    rate: f64,
    priced_at: &str,
    source: &str,
) -> Result<bool> {
    let existing_source: Option<String> = conn
        .query_row(
            "SELECT source FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
            rusqlite::params![base_code, quote_code],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    if existing_source.as_deref() == Some(MANUAL_SOURCE) {
        return Ok(false);
    }
    write_exchange_rate(
        conn,
        &new_uuid(),
        base_code,
        quote_code,
        rate,
        priced_at,
        Some(source),
    )?;
    Ok(true)
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
    ensure_transaction(conn, || {
        if input.price_cents <= 0 {
            return Err(AppError::coded(
                "instrument.price-positive",
                "价格必须大于 0",
            ));
        }
        let id = upsert_market_price(
            conn,
            &MarketPriceWrite {
                instrument_id: &input.instrument_id,
                price_cents: input.price_cents,
                currency_code: &input.currency_code,
                priced_at: &input.priced_at,
                nav_date: None,
                source: input.source.as_deref(),
            },
        )?;
        record_price(
            conn,
            PriceCommand::MarketPrice {
                instrument_id: input.instrument_id,
                price_cents: input.price_cents,
                currency_code: input.currency_code,
                priced_at: input.priced_at,
                source: input.source,
            },
        )?;
        Ok(id)
    })
}

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
    if let Some(kind) = filter.kind {
        params.push(Box::new(kind.to_string()));
        conditions.push(format!("i.instrument_type=?{}", params.len()));
    }
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
            "SELECT {} \
             FROM instruments i \
             LEFT JOIN market_prices p ON p.instrument_id = i.id \
             {where_clause} ORDER BY i.symbol{limit_clause}",
            instrument_row_projection()
        )
    };

    let page_size = filter.page_size.unwrap_or(50).clamp(1, 500);

    let anchor_page_in_sql_branch = match filter.anchor_id.as_deref() {
        Some(anchor_id) => {
            let id_sql = format!("SELECT i.id FROM instruments i{where_clause} ORDER BY i.symbol");
            let mut located = None;
            let mut stmt = conn.prepare(&id_sql)?;
            let rows = stmt.query_map(params_ref.as_slice(), |r| r.get::<_, String>(0))?;
            for (idx, id) in rows.flatten().enumerate() {
                if id == anchor_id {
                    located = Some(idx / page_size + 1);
                    break;
                }
            }
            located
        }
        None => None,
    };
    let (total, items, anchor_page) = if let Some(terms) = &search_terms {
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
        let anchor_page = filter.anchor_id.as_deref().and_then(|anchor_id| {
            matched
                .iter()
                .position(|inst| inst.id == anchor_id)
                .map(|idx| idx / page_size + 1)
        });
        let page = anchor_page.unwrap_or_else(|| filter.page.unwrap_or(1).max(1));
        let offset = (page - 1) * page_size;
        let items = matched.into_iter().skip(offset).take(page_size).collect();
        (total, items, anchor_page)
    } else {
        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM instruments i{where_clause}"),
            params_ref.as_slice(),
            |r| r.get(0),
        )?;
        let page = anchor_page_in_sql_branch.unwrap_or_else(|| filter.page.unwrap_or(1).max(1));
        let offset = (page - 1) * page_size;
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
        (total, items, anchor_page_in_sql_branch)
    };

    Ok(InstrumentListResult {
        items,
        total,
        anchor_page,
    })
}

fn instrument_row_projection() -> String {
    format!(
        "i.id,i.symbol,i.instrument_type,i.name,i.currency_code,i.market,i.created_at,i.updated_at,i.version,i.device_id,i.source,p.price_cents, \
         CASE WHEN {INVESTED_EXISTS} THEN 1 ELSE 0 END AS invested, i.constant_unit_price"
    )
}

pub fn delete_instrument(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        write_delete_instrument(conn, id)?;
        record_instrument(conn, InstrumentCommand::Delete { id: id.to_string() })
    })
}

pub(crate) fn write_delete_instrument(conn: &Connection, id: &str) -> Result<()> {
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
    if source != MANUAL_SOURCE {
        return Err(AppError::coded(
            "instrument.sync-delete-forbidden",
            "同步来源标的不支持删除：名称与市场由按代码查询/创建带回权威信息维护",
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

pub fn create_instrument(conn: &Connection, input: InstrumentInput) -> Result<String> {
    ensure_transaction(conn, || {
        if input.symbol.trim().is_empty() {
            return Err(AppError::coded(
                "instrument.symbol-required",
                "标的代码不能为空",
            ));
        }
        let row = InstrumentCommandRow {
            symbol: input.symbol,
            kind: input.kind,
            name: input.name,
            currency_code: input.currency_code,
            market: input.market.unwrap_or_else(|| FUND_MARKET.to_string()),
        };
        let (id, outcome) = write_instrument(conn, &new_uuid(), &row)?;
        match outcome {
            InstrumentWrite::Created => record_instrument(
                conn,
                InstrumentCommand::Create {
                    id: id.clone(),
                    row: row.clone(),
                },
            )?,
            InstrumentWrite::Renamed => record_instrument(
                conn,
                InstrumentCommand::Update {
                    id: id.clone(),
                    symbol: row.symbol.clone(),
                    kind: row.kind,
                    name: row.name.clone(),
                    market: row.market.clone(),
                },
            )?,
            InstrumentWrite::Unchanged => {}
        }
        Ok(id)
    })
}

pub(crate) enum InstrumentWrite {
    Created,
    Renamed,
    Unchanged,
}

fn ensure_fund_market_unknown(kind: InstrumentType, market: &str) -> Result<()> {
    if kind == InstrumentType::Fund {
        reject_non_unknown_fund_market(market)?;
    }
    Ok(())
}

pub(crate) fn write_instrument(
    conn: &Connection,
    insert_id: &str,
    row: &InstrumentCommandRow,
) -> Result<(String, InstrumentWrite)> {
    ensure_fund_market_unknown(row.kind, &row.market)?;
    let existing_id: Option<(String, Option<String>, String)> = conn
        .query_row(
            "SELECT id, name, market FROM instruments WHERE symbol=?1 AND instrument_type=?2",
            rusqlite::params![row.symbol, row.kind],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    if let Some((existing_id, existing_name, existing_market)) = existing_id {
        let name_changed = row.name != existing_name;
        let market_changed = row.market != existing_market;
        if name_changed || market_changed {
            let now = now_iso();
            conn.execute(
                "UPDATE instruments SET name=?1, market=?2, updated_at=?3, version=version+1 WHERE id=?4",
                rusqlite::params![row.name, row.market, now, existing_id],
            )?;
            return Ok((existing_id, InstrumentWrite::Renamed));
        }
        return Ok((existing_id, InstrumentWrite::Unchanged));
    }
    let now = now_iso();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id,source) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        rusqlite::params![
            insert_id,
            row.symbol,
            row.kind,
            row.name,
            row.currency_code,
            row.market,
            now,
            now,
            1,
            device_id(conn)?,
            MANUAL_SOURCE
        ],
    )?;
    Ok((insert_id.to_string(), InstrumentWrite::Created))
}

pub(crate) fn write_instrument_update(
    conn: &Connection,
    symbol: &str,
    kind: InstrumentType,
    name: Option<&str>,
    market: &str,
) -> Result<()> {
    ensure_fund_market_unknown(kind, market)?;
    let changed = conn.execute(
        "UPDATE instruments SET name=?1, market=?2, updated_at=?3, version=version+1 \
         WHERE symbol=?4 AND instrument_type=?5",
        rusqlite::params![name, market, now_iso(), symbol, kind],
    )?;
    if changed == 0 {
        return Err(AppError::codedp_not_found(
            "instrument.not-found",
            format!("标的 {symbol} 不存在"),
            &[symbol],
        ));
    }
    Ok(())
}

pub fn refresh_instrument_name(conn: &Connection, instrument_id: &str, name: &str) -> Result<bool> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(false);
    }
    let current: Option<String> = match conn.query_row(
        "SELECT name FROM instruments WHERE id=?1",
        rusqlite::params![instrument_id],
        |r| r.get(0),
    ) {
        Ok(name) => Some(name),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => return Err(e.into()),
    };
    match current {
        Some(existing) if existing != name => {
            conn.execute(
                "UPDATE instruments SET name=?1, updated_at=?2, version=version+1 WHERE id=?3",
                rusqlite::params![name, now_iso(), instrument_id],
            )?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub fn create_instrument_manual(conn: &Connection, input: InstrumentInput) -> Result<String> {
    match input.kind {
        InstrumentType::Bond | InstrumentType::Etf | InstrumentType::Other => {}
        InstrumentType::Stock => {
            return Err(AppError::coded(
                "instrument.stock-manual-forbidden",
                "股票类标的不支持手动创建：请用「添加投资标的」按代码查询，自动回填权威名称",
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
