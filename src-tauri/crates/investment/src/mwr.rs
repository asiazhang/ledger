use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::NaiveDate;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::as_of::AsOfValues;
use super::holdings::holdings_as_of_in;
use super::lots::QTY_GUARD_EPSILON;
use super::staleness::beijing_today;
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::{AppError, Result};

pub fn xirr(flows: &[(f64, f64)]) -> Option<f64> {
    let flows: Vec<(f64, f64)> = flows.iter().copied().filter(|(_, cf)| *cf != 0.0).collect();
    let has_neg = flows.iter().any(|(_, cf)| *cf < 0.0);
    let has_pos = flows.iter().any(|(_, cf)| *cf > 0.0);
    if flows.len() < 2 || !has_neg || !has_pos {
        return None;
    }
    let t_max = flows
        .iter()
        .map(|(t, _)| *t)
        .fold(f64::NEG_INFINITY, f64::max);
    if t_max.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return None;
    }
    let npv = |r: f64| -> f64 { flows.iter().map(|(t, cf)| cf / (1.0 + r).powf(*t)).sum() };

    let mut lo = -1.0 + 1e-9;
    let mut hi = 1e9f64;
    let mut f_lo = npv(lo);
    let f_hi = npv(hi);
    if f_lo == 0.0 {
        return Some(lo);
    }
    if f_hi == 0.0 {
        return Some(hi);
    }
    if (f_lo > 0.0) == (f_hi > 0.0) {
        return None;
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if mid <= lo || mid >= hi {
            break;
        }
        let f_mid = npv(mid);
        if f_mid == 0.0 {
            return Some(mid);
        }
        if (f_mid > 0.0) != (f_lo > 0.0) {
            hi = mid;
        } else {
            lo = mid;
            f_lo = f_mid;
        }
    }
    Some(0.5 * (lo + hi))
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MwrRange {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MwrBasis {
    Annualized,
    Cumulative,
}

#[derive(Debug, Serialize)]
pub struct InstrumentMwr {
    pub account_id: String,
    pub instrument_id: String,
    pub currency_code: String,
    pub basis: MwrBasis,
    pub rate: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct AccountMwr {
    pub account_id: String,
    pub account_name: String,
    pub currency_code: String,
    pub basis: MwrBasis,
    pub rate: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct CurrencyMwr {
    pub currency_code: String,
    pub basis: MwrBasis,
    pub rate: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct MoneyWeightedReturnSummary {
    pub by_instrument: Vec<InstrumentMwr>,
    pub by_account: Vec<AccountMwr>,
    pub total: Vec<CurrencyMwr>,
}

pub fn query_money_weighted_return_summary(
    conn: &Connection,
    range: &MwrRange,
) -> Result<MoneyWeightedReturnSummary> {
    query_money_weighted_return_summary_on(conn, range, beijing_today())
}

pub fn query_money_weighted_return_summary_on(
    conn: &Connection,
    range: &MwrRange,
    today: NaiveDate,
) -> Result<MoneyWeightedReturnSummary> {
    ensure_transaction(conn, || {
        query_money_weighted_return_summary_within_tx(conn, range, today)
    })
}

fn query_money_weighted_return_summary_within_tx(
    conn: &Connection,
    range: &MwrRange,
    today: NaiveDate,
) -> Result<MoneyWeightedReturnSummary> {
    let (start, end) = validate_range(range)?;

    let mut pairs: BTreeMap<(String, String), PairFlows> = BTreeMap::new();
    load_cash_flows(conn, start, end, &mut pairs)?;

    apply_boundaries(conn, start, end, today, &mut pairs)?;

    let by_instrument: Vec<InstrumentMwr> = pairs
        .iter()
        .filter(|(_, p)| p.current_position && !p.unvalued)
        .map(|(key, p)| {
            let (basis, rate) = p.measure();
            InstrumentMwr {
                account_id: key.0.clone(),
                instrument_id: key.1.clone(),
                currency_code: p.currency_code.clone(),
                basis,
                rate,
            }
        })
        .collect();

    let accounts: Vec<AccountRow> = query_all(
        conn,
        "SELECT id, name, currency_code, type FROM accounts WHERE is_deleted = 0",
        [],
    )?;
    let mut by_account: Vec<AccountMwr> = accounts
        .into_iter()
        .filter(|a| a.is_investment)
        .filter_map(|a| {
            let owned: Vec<&PairFlows> = pairs
                .iter()
                .filter(|(key, p)| !p.unvalued && key.0 == a.id)
                .map(|(_, p)| p)
                .collect();
            if owned.is_empty() {
                return None;
            }
            let (basis, rate) = measure_collection(&owned);
            Some(AccountMwr {
                account_id: a.id,
                account_name: a.name,
                currency_code: a.currency_code,
                basis,
                rate,
            })
        })
        .collect();
    by_account.sort_by(|x, y| {
        x.account_name
            .cmp(&y.account_name)
            .then(x.currency_code.cmp(&y.currency_code))
    });

    let mut by_currency: BTreeMap<String, Vec<&PairFlows>> = BTreeMap::new();
    for p in pairs.values() {
        if p.unvalued {
            continue;
        }
        by_currency
            .entry(p.currency_code.clone())
            .or_default()
            .push(p);
    }
    let total: Vec<CurrencyMwr> = by_currency
        .into_iter()
        .map(|(currency_code, group)| {
            let (basis, rate) = measure_collection(&group);
            CurrencyMwr {
                currency_code,
                basis,
                rate,
            }
        })
        .collect();

    Ok(MoneyWeightedReturnSummary {
        by_instrument,
        by_account,
        total,
    })
}

fn validate_range(range: &MwrRange) -> Result<(Option<NaiveDate>, Option<NaiveDate>)> {
    let parse = |label: &str, raw: &Option<String>| -> Result<Option<NaiveDate>> {
        raw.as_deref()
            .map(|s| {
                NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
                    AppError::codedp(
                        "instrument.mwr-date-invalid",
                        format!("{label}日期格式无效: {s}"),
                        &[s],
                    )
                })
            })
            .transpose()
    };
    let start = parse("起始", &range.start_date)?;
    let end = parse("截止", &range.end_date)?;
    if let (Some(s), Some(e)) = (start, end)
        && s > e
    {
        return Err(AppError::coded(
            "instrument.mwr-range-invalid",
            "起始日期不能晚于截止日期",
        ));
    }
    Ok((start, end))
}

struct FlowRow {
    account_id: String,
    date: String,
    kind: String,
    amount_cents: i64,
    instrument_id: String,
    to_instrument_id: Option<String>,
    origin: String,
}

impl FromRow for FlowRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(FlowRow {
            account_id: row.get(0)?,
            date: row.get(1)?,
            kind: row.get(2)?,
            amount_cents: row.get(3)?,
            instrument_id: row.get(4)?,
            to_instrument_id: row.get(5)?,
            origin: row.get(6)?,
        })
    }
}

fn load_cash_flows(
    conn: &Connection,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
    pairs: &mut BTreeMap<(String, String), PairFlows>,
) -> Result<()> {
    let sql = "SELECT t.account_id, t.date, t.kind, t.amount_cents, st.instrument_id, \
               st.to_instrument_id, st.origin \
               FROM transactions t \
               JOIN security_transactions st ON st.transaction_id = t.id \
               JOIN accounts a ON a.id = t.account_id \
               WHERE t.kind IN ('buy','sell','dividend','convert') \
                 AND t.is_deleted = 0 AND a.is_deleted = 0 \
               ORDER BY t.date";
    let rows: Vec<FlowRow> = query_all(conn, sql, [])?;
    let opening_pairs: BTreeSet<(String, String)> = rows
        .iter()
        .filter(|r| r.origin == OPENING_ORIGIN)
        .map(|r| (r.account_id.clone(), r.instrument_id.clone()))
        .collect();
    let mut first_real: BTreeMap<(String, String), NaiveDate> = BTreeMap::new();
    for row in &rows {
        if row.origin == OPENING_ORIGIN {
            continue;
        }
        let date = parse_date(&row.date)?;
        let mut legs = vec![(row.account_id.clone(), row.instrument_id.clone())];
        if let Some(to) = &row.to_instrument_id {
            legs.push((row.account_id.clone(), to.clone()));
        }
        for key in legs {
            first_real
                .entry(key)
                .and_modify(|d| {
                    if date < *d {
                        *d = date;
                    }
                })
                .or_insert(date);
        }
    }
    first_real.retain(|key, _| opening_pairs.contains(key));

    for row in rows {
        let date = parse_date(&row.date)?;
        let out_key = (row.account_id.clone(), row.instrument_id.clone());
        let in_key = row
            .to_instrument_id
            .clone()
            .map(|to| (row.account_id.clone(), to));
        if row.origin == OPENING_ORIGIN {
            let pair = pairs.entry(out_key).or_default();
            pair.has_opening = true;
            pair.opening_invested_cents += row.amount_cents;
            continue;
        }
        let lifetime = opening_pairs.contains(&out_key)
            || in_key.as_ref().is_some_and(|k| opening_pairs.contains(k));
        let in_window = if row.kind == "dividend" {
            start.is_none_or(|s| date >= s) && in_range(date, None, end)
        } else {
            in_range(date, start, end)
        };
        if !lifetime && !in_window {
            continue;
        }
        if row.kind == "convert" {
            let out_leg = row.instrument_id.clone();
            if !route_t0_day_flow(
                pairs,
                &out_key,
                date,
                &row.kind,
                row.amount_cents as f64,
                &first_real,
            ) {
                pairs
                    .entry((row.account_id.clone(), out_leg))
                    .or_default()
                    .push(date, row.amount_cents as f64);
            }
            if let Some(in_key) = in_key {
                let in_leg_amount = -(row.amount_cents as f64);
                if !route_t0_day_flow(pairs, &in_key, date, &row.kind, in_leg_amount, &first_real) {
                    pairs.entry(in_key).or_default().push(date, in_leg_amount);
                }
            }
            continue;
        }
        let signed = match row.kind.as_str() {
            "buy" => -(row.amount_cents as f64),
            "sell" | "dividend" => row.amount_cents as f64,
            _ => continue,
        };
        if !route_t0_day_flow(pairs, &out_key, date, &row.kind, signed, &first_real) {
            pairs
                .entry((row.account_id.clone(), row.instrument_id.clone()))
                .or_default()
                .push(date, signed);
        }
    }
    Ok(())
}

fn route_t0_day_flow(
    pairs: &mut BTreeMap<(String, String), PairFlows>,
    key: &(String, String),
    date: NaiveDate,
    kind: &str,
    amount: f64,
    first_real: &BTreeMap<(String, String), NaiveDate>,
) -> bool {
    if first_real.get(key) != Some(&date) {
        return false;
    }
    let pair = pairs.entry(key.clone()).or_default();
    pair.first_real_flow = Some(date);
    if kind == "dividend" {
        return false;
    }
    pair.t0_day_cost_flows.push((date, amount));
    true
}

#[derive(Default)]
struct PairFlows {
    flows: Vec<(NaiveDate, f64)>,
    t0_day_cost_flows: Vec<(NaiveDate, f64)>,
    t0_fold: Option<(NaiveDate, f64)>,
    opening_invested_cents: i64,
    has_opening: bool,
    first_real_flow: Option<NaiveDate>,
    currency_code: String,
    current_position: bool,
    unvalued: bool,
}

impl PairFlows {
    fn opening_only(&self) -> bool {
        self.has_opening && self.first_real_flow.is_none()
    }

    fn push(&mut self, date: NaiveDate, amount: f64) {
        self.flows.push((date, amount));
    }

    fn measure(&self) -> (MwrBasis, Option<f64>) {
        if self.opening_only() {
            (MwrBasis::Cumulative, self.cumulative_rate())
        } else {
            (MwrBasis::Annualized, self.annualized_rate())
        }
    }

    fn annualized_rate(&self) -> Option<f64> {
        match &self.t0_fold {
            None => rate_of(&self.flows),
            Some(fold) => {
                let mut all = Vec::with_capacity(self.flows.len() + 1);
                all.extend(self.flows.iter().copied());
                all.push(*fold);
                rate_of(&all)
            }
        }
    }

    fn annualized_flows(&self) -> impl Iterator<Item = (NaiveDate, f64)> + '_ {
        self.flows
            .iter()
            .copied()
            .chain(self.t0_fold.iter().copied())
    }

    fn cumulative_profit(&self) -> f64 {
        let opening = self.opening_invested_cents as f64;
        self.cost_flows().map(|(_, cf)| *cf).sum::<f64>() - opening
    }

    fn cumulative_invested(&self) -> f64 {
        self.opening_invested_cents as f64
            + self
                .cost_flows()
                .filter(|(_, cf)| *cf < 0.0)
                .map(|(_, cf)| -*cf)
                .sum::<f64>()
    }

    fn cost_flows(&self) -> impl Iterator<Item = &(NaiveDate, f64)> {
        self.flows.iter().chain(self.t0_day_cost_flows.iter())
    }

    fn cumulative_rate(&self) -> Option<f64> {
        aggregate_cumulative_rate(std::slice::from_ref(&self))
    }
}

fn measure_collection(pairs: &[&PairFlows]) -> (MwrBasis, Option<f64>) {
    if pairs.iter().any(|p| p.opening_only()) {
        (MwrBasis::Cumulative, aggregate_cumulative_rate(pairs))
    } else {
        let flows: Vec<(NaiveDate, f64)> =
            pairs.iter().flat_map(|p| p.annualized_flows()).collect();
        (MwrBasis::Annualized, rate_of(&flows))
    }
}

fn aggregate_cumulative_rate(pairs: &[&PairFlows]) -> Option<f64> {
    let invested: f64 = pairs.iter().map(|p| p.cumulative_invested()).sum();
    if invested <= 0.0 {
        return None;
    }
    let profit: f64 = pairs.iter().map(|p| p.cumulative_profit()).sum();
    Some(profit / invested)
}

fn rate_of(flows: &[(NaiveDate, f64)]) -> Option<f64> {
    let first = flows.iter().map(|(d, _)| *d).min()?;
    let converted: Vec<(f64, f64)> = flows
        .iter()
        .map(|(d, cf)| {
            (
                (d.signed_duration_since(first).num_days() as f64) / 365.0,
                *cf,
            )
        })
        .collect();
    xirr(&converted)
}

fn parse_date(raw: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
        AppError::codedp(
            "instrument.mwr-date-invalid",
            format!("流水日期格式无效: {raw}"),
            &[raw],
        )
    })
}

const OPENING_ORIGIN: &str = "opening";

fn in_range(date: NaiveDate, start: Option<NaiveDate>, end: Option<NaiveDate>) -> bool {
    if start.is_some_and(|s| date <= s) {
        return false;
    }
    if end.is_some_and(|e| date > e) {
        return false;
    }
    true
}

struct AccountRow {
    id: String,
    name: String,
    currency_code: String,
    is_investment: bool,
}

impl FromRow for AccountRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let kind: String = row.get(3)?;
        Ok(AccountRow {
            id: row.get(0)?,
            name: row.get(1)?,
            currency_code: row.get(2)?,
            is_investment: kind == "investment",
        })
    }
}

fn apply_boundaries(
    conn: &Connection,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
    today: NaiveDate,
    pairs: &mut BTreeMap<(String, String), PairFlows>,
) -> Result<()> {
    let mut current: HashMap<(String, String), (Option<i64>, String)> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT v.account_id, v.instrument_id, SUM(v.market_value_cents), a.currency_code \
             FROM v_holdings v JOIN accounts a ON a.id = v.account_id \
             GROUP BY v.account_id, v.instrument_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (account_id, instrument_id, value, currency) = row?;
            current.insert((account_id, instrument_id), (value, currency));
        }
    }

    let start_boundary = match start {
        Some(date) => Some((date, AsOfValues::load(conn, date)?)),
        None => None,
    };
    let end_boundary = match end {
        Some(date) => Some((date, AsOfValues::load(conn, date)?)),
        None => None,
    };
    let end_date = end.unwrap_or(today);
    let mut fold_values: HashMap<NaiveDate, AsOfValues> = HashMap::new();
    for t0 in pairs
        .values()
        .filter_map(|p| p.first_real_flow)
        .collect::<BTreeSet<_>>()
    {
        fold_values.insert(t0, AsOfValues::load(conn, t0)?);
    }

    let mut keys: Vec<(String, String)> = pairs.keys().cloned().collect();
    for key in current.keys() {
        if !pairs.contains_key(key) {
            keys.push(key.clone());
        }
    }
    for key in keys {
        let pair = pairs.entry(key.clone()).or_default();
        if let Some((_, currency)) = current.get(&key) {
            pair.currency_code = currency.clone();
        }
        if pair.currency_code.is_empty() {
            pair.currency_code = account_currency_of(conn, &key.0)?;
        }

        if pair.has_opening {
            if let Some((value, _)) = current.get(&key) {
                pair.current_position = true;
                match value {
                    Some(v) => pair.flows.push((today, *v as f64)),
                    None => pair.unvalued = true,
                }
            }
            if let Some(t0) = pair.first_real_flow {
                let quantity =
                    holdings_as_of_in(conn, Some(&key.0), Some(&key.1), &t0.to_string())?;
                if quantity > QTY_GUARD_EPSILON {
                    match fold_values[&t0].market_value(quantity, &key.1, &pair.currency_code) {
                        Some(v) => pair.t0_fold = Some((t0, -(v as f64))),
                        None => pair.unvalued = true,
                    }
                }
            }
            continue;
        }

        if let Some((start_date, values)) = &start_boundary {
            let quantity =
                holdings_as_of_in(conn, Some(&key.0), Some(&key.1), &start_date.to_string())?;
            if quantity > QTY_GUARD_EPSILON {
                match values.market_value(quantity, &key.1, &pair.currency_code) {
                    Some(value) => pair.flows.push((*start_date, -(value as f64))),
                    None => {
                        pair.unvalued = true;
                        continue;
                    }
                }
            }
        }

        if let Some((boundary_date, values)) = &end_boundary {
            let quantity =
                holdings_as_of_in(conn, Some(&key.0), Some(&key.1), &boundary_date.to_string())?;
            if quantity > QTY_GUARD_EPSILON {
                pair.current_position = true;
                match values.market_value(quantity, &key.1, &pair.currency_code) {
                    Some(value) => pair.flows.push((*boundary_date, value as f64)),
                    None => pair.unvalued = true,
                }
            }
        } else if let Some((value, _)) = current.get(&key) {
            pair.current_position = true;
            match value {
                Some(v) => pair.flows.push((end_date, *v as f64)),
                None => pair.unvalued = true,
            }
        }
    }
    Ok(())
}

fn account_currency_of(conn: &Connection, account_id: &str) -> Result<String> {
    conn.query_row(
        "SELECT currency_code FROM accounts WHERE id = ?1 AND is_deleted = 0",
        [account_id],
        |r| r.get(0),
    )
    .map_err(|_| {
        AppError::codedp(
            "instrument.mwr-account-missing",
            format!("收益现金流指向的账户不存在或已删除: {account_id}"),
            &[account_id],
        )
    })
}
