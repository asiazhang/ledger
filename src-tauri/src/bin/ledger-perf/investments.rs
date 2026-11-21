use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::Connection;

use ledger_investment::MANUAL_SOURCE;

use super::generate::date_millis;
use super::generate::{DEVICE_ID, GenCounts, Window};
use super::rng::{Rng, time_ordered_id};

pub(crate) const INSTRUMENT_TOTAL: usize = 20;
const FULL_SELL_RATE: f64 = 0.25;
const PARTIAL_SELL_MIN: f64 = 0.30;
const PARTIAL_SELL_SPAN: f64 = 0.40;
const STOCK_FEE_RATE: f64 = 0.000_3;
const FUND_BUY_FEE_RATE: f64 = 0.001_5;
const WEEK_DRIFT: f64 = 0.06;
const MIN_PRICE_UNITS: i64 = 1_000;
const PRICE_UNITS_PER_FEN: f64 = 100.0;

pub(crate) struct InstrumentSpec {
    pub symbol: &'static str,
    pub itype: &'static str,
    pub name: &'static str,
    pub ccy: &'static str,
    pub market: &'static str,
    pub source: &'static str,
    pub base_price: i64,
}

pub(crate) const INSTRUMENTS: [InstrumentSpec; INSTRUMENT_TOTAL] = [
    InstrumentSpec {
        symbol: "600519.SH",
        itype: "stock",
        name: "贵州茅台",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 14_000_000,
    },
    InstrumentSpec {
        symbol: "300750.SZ",
        itype: "stock",
        name: "宁德时代",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 2_500_000,
    },
    InstrumentSpec {
        symbol: "600036.SH",
        itype: "stock",
        name: "招商银行",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 350_000,
    },
    InstrumentSpec {
        symbol: "600900.SH",
        itype: "stock",
        name: "长江电力",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 280_000,
    },
    InstrumentSpec {
        symbol: "601318.SH",
        itype: "stock",
        name: "中国平安",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 500_000,
    },
    InstrumentSpec {
        symbol: "000858.SZ",
        itype: "stock",
        name: "五粮液",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 1_200_000,
    },
    InstrumentSpec {
        symbol: "000333.SZ",
        itype: "stock",
        name: "美的集团",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 700_000,
    },
    InstrumentSpec {
        symbol: "000651.SZ",
        itype: "stock",
        name: "格力电器",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 400_000,
    },
    InstrumentSpec {
        symbol: "002594.SZ",
        itype: "stock",
        name: "比亚迪",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 2_500_000,
    },
    InstrumentSpec {
        symbol: "601012.SH",
        itype: "stock",
        name: "隆基绿能",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 200_000,
    },
    InstrumentSpec {
        symbol: "002415.SZ",
        itype: "stock",
        name: "海康威视",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 300_000,
    },
    InstrumentSpec {
        symbol: "600030.SH",
        itype: "stock",
        name: "中信证券",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 250_000,
    },
    InstrumentSpec {
        symbol: "00700.HK",
        itype: "stock",
        name: "腾讯控股",
        ccy: "HKD",
        market: "hk",
        source: "eastmoney",
        base_price: 3_800_000,
    },
    InstrumentSpec {
        symbol: "03690.HK",
        itype: "stock",
        name: "美团-W",
        ccy: "HKD",
        market: "hk",
        source: "eastmoney",
        base_price: 1_200_000,
    },
    InstrumentSpec {
        symbol: "01810.HK",
        itype: "stock",
        name: "小米集团-W",
        ccy: "HKD",
        market: "hk",
        source: "eastmoney",
        base_price: 200_000,
    },
    InstrumentSpec {
        symbol: "510300.SH",
        itype: "etf",
        name: "沪深300ETF",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 40_000,
    },
    InstrumentSpec {
        symbol: "510500.SH",
        itype: "etf",
        name: "中证500ETF",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 60_000,
    },
    InstrumentSpec {
        symbol: "005827",
        itype: "fund",
        name: "易方达蓝筹精选",
        ccy: "CNY",
        market: "unknown",
        source: MANUAL_SOURCE,
        base_price: 25_000,
    },
    InstrumentSpec {
        symbol: "003095",
        itype: "fund",
        name: "中欧医疗健康",
        ccy: "CNY",
        market: "unknown",
        source: MANUAL_SOURCE,
        base_price: 18_000,
    },
    InstrumentSpec {
        symbol: "000961",
        itype: "fund",
        name: "天弘沪深300ETF联接A",
        ccy: "CNY",
        market: "unknown",
        source: MANUAL_SOURCE,
        base_price: 15_000,
    },
];

fn is_fund(itype: &str) -> bool {
    itype == "fund"
}

pub(crate) struct InstrumentRow {
    pub id: String,
    pub ccy: &'static str,
}

pub(crate) fn insert_instruments(
    conn: &Connection,
    stamp: &str,
    millis: i64,
    counts: &mut GenCounts,
) -> Result<Vec<InstrumentRow>, String> {
    let sql = "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,\
         created_at,updated_at,version,device_id,source) VALUES (?1,?2,?3,?4,?5,?6,?7,?7,1,?8,?9)";
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let mut rows = Vec::with_capacity(INSTRUMENT_TOTAL);
    for (seq, spec) in INSTRUMENTS.iter().enumerate() {
        let id = time_ordered_id("instruments", seq as u64, millis);
        stmt.execute(rusqlite::params![
            id,
            spec.symbol,
            spec.itype,
            spec.name,
            spec.ccy,
            spec.market,
            stamp,
            DEVICE_ID,
            spec.source,
        ])
        .map_err(|e| e.to_string())?;
        rows.push(InstrumentRow { id, ccy: spec.ccy });
    }
    counts.instruments = rows.len();
    Ok(rows)
}

pub(crate) struct MarketData {
    start: NaiveDate,
    walks: Vec<Vec<i64>>,
}

impl MarketData {
    pub fn price_at(&self, instrument_idx: usize, date: NaiveDate) -> i64 {
        let base = INSTRUMENTS[instrument_idx].base_price;
        let weeks = self.walks[instrument_idx].len();
        if weeks == 0 {
            return base;
        }
        let raw = (date - self.start).num_days() / 7;
        let idx = raw.clamp(0, weeks as i64 - 1) as usize;
        self.walks[instrument_idx][idx]
    }
}

pub(crate) fn insert_market_data(
    conn: &Connection,
    rng: &mut Rng,
    instruments: &[InstrumentRow],
    window: Window,
    counts: &mut GenCounts,
) -> Result<MarketData, String> {
    let start = window.start;
    let mut monday = start - Duration::days(i64::from(start.weekday().num_days_from_monday()));
    let mut week_dates = Vec::new();
    while monday <= window.end {
        week_dates.push((monday + Duration::days(4)).min(window.end));
        monday += Duration::days(7);
    }

    let history_sql = "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,\
         currency_code,source,created_at,updated_at,version,device_id)\
         VALUES (?1,?2,?3,?4,?5,'eastmoney',?6,?6,1,?7)";
    let mut history = conn.prepare(history_sql).map_err(|e| e.to_string())?;
    let price_sql = "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,\
         priced_at,nav_date,source,created_at,updated_at,version,device_id)\
         VALUES (?1,?2,?3,?4,?5,?6,'eastmoney',?7,?7,1,?8)";
    let mut price_stmt = conn.prepare(price_sql).map_err(|e| e.to_string())?;

    let end_millis = date_millis(&window.end);
    let mut walks = Vec::with_capacity(instruments.len());
    let mut seq = 0u64;
    let mut history_rows = 0usize;
    for (idx, row) in instruments.iter().enumerate() {
        let spec = &INSTRUMENTS[idx];
        let mut walk = Vec::with_capacity(week_dates.len());
        let mut price = spec.base_price;
        for week_date in &week_dates {
            let drifted = (price as f64 * (1.0 + (rng.next_f64() - 0.5) * WEEK_DRIFT)).round();
            price = (drifted as i64).max(MIN_PRICE_UNITS);
            walk.push(price);
            let created = format!("{week_date}T16:00:00Z");
            history
                .execute(rusqlite::params![
                    time_ordered_id("price_history", seq, date_millis(week_date)),
                    row.id,
                    week_date.to_string(),
                    price,
                    row.ccy,
                    created,
                    DEVICE_ID,
                ])
                .map_err(|e| e.to_string())?;
            seq += 1;
            history_rows += 1;
        }
        let last_date = week_dates[week_dates.len() - 1];
        let nav_date = if is_fund(spec.itype) {
            Some(last_date.to_string())
        } else {
            None
        };
        price_stmt
            .execute(rusqlite::params![
                time_ordered_id("market_prices", idx as u64, end_millis),
                row.id,
                price,
                row.ccy,
                last_date.to_string(),
                nav_date,
                format!("{}T16:00:00Z", last_date),
                DEVICE_ID,
            ])
            .map_err(|e| e.to_string())?;
        walks.push(walk);
    }
    counts.market_prices = instruments.len();
    counts.price_history = history_rows;
    Ok(MarketData { start, walks })
}

struct LotState {
    id: String,
    created: String,
    buy_date: NaiveDate,
    initial: f64,
    remaining: f64,
    cost_per_unit: i64,
    cost_allocated: i64,
}

pub(crate) struct Portfolio {
    pairs: Vec<Vec<LotState>>,
    next_lot_seq: u64,
    next_sale_seq: u64,
    buys: usize,
    sells: usize,
}

impl Portfolio {
    pub fn new(slot_count: usize) -> Self {
        let pairs = (0..slot_count * INSTRUMENT_TOTAL)
            .map(|_| Vec::new())
            .collect();
        Portfolio {
            pairs,
            next_lot_seq: 0,
            next_sale_seq: 0,
            buys: 0,
            sells: 0,
        }
    }

    pub fn trade_counts(&self) -> (usize, usize) {
        (self.buys, self.sells)
    }
}

pub(crate) struct BuyTrade {
    pub slot: usize,
    pub instrument_idx: usize,
    pub ccy: &'static str,
    pub quantity: f64,
    pub price_units: i64,
    pub fee_cents: i64,
    pub amount_cents: i64,
    pub cost_per_unit: i64,
    pub date: NaiveDate,
}

pub(crate) struct SellTrade {
    pub slot: usize,
    pub instrument_idx: usize,
    pub ccy: &'static str,
    pub quantity: f64,
    pub price_units: i64,
    pub fee_cents: i64,
    pub amount_cents: i64,
    gross_cents: i64,
    pub date: NaiveDate,
    matches: Vec<LotMatch>,
}

struct LotMatch {
    pos: usize,
    quantity: f64,
    cost_per_unit: i64,
    initial: f64,
    exhausts: bool,
}

pub(crate) fn plan_buy(
    rng: &mut Rng,
    ccy_slots: &[(&'static str, Vec<usize>)],
    md: &MarketData,
    window: Window,
) -> Option<BuyTrade> {
    let inst = rng.below(INSTRUMENT_TOTAL as u64) as usize;
    let spec = &INSTRUMENTS[inst];
    let slots = ccy_slots
        .iter()
        .find(|(c, _)| *c == spec.ccy)
        .map(|(_, s)| s)?;
    let slot = *rng.pick(slots);
    let date = window.rand_date(rng);
    let price = md.price_at(inst, date);

    let (quantity, fee, amount, price_units, cost_per_unit) = if is_fund(spec.itype) {
        let amount = rng.range_i64(500, 5_000) * 100;
        let fee = (amount as f64 * FUND_BUY_FEE_RATE).round() as i64;
        let quantity =
            (((amount - fee) as f64 * PRICE_UNITS_PER_FEN / price as f64) * 100.0).floor() / 100.0;
        if quantity < 0.01 {
            return None;
        }
        let derived = ((amount - fee) as f64 * PRICE_UNITS_PER_FEN / quantity).round() as i64;
        let cost = (amount as f64 * PRICE_UNITS_PER_FEN / quantity).round() as i64;
        (quantity, fee, amount, derived, cost)
    } else {
        let quantity = ((1 + rng.below(2)) * 100) as f64;
        let gross = (quantity * price as f64 / PRICE_UNITS_PER_FEN).round() as i64;
        let fee = (gross as f64 * STOCK_FEE_RATE).round() as i64;
        let amount = gross + fee;
        let cost = ((quantity * price as f64 + fee as f64 * PRICE_UNITS_PER_FEN) / quantity).round()
            as i64;
        (quantity, fee, amount, price, cost)
    };

    Some(BuyTrade {
        slot,
        instrument_idx: inst,
        ccy: spec.ccy,
        quantity,
        price_units,
        fee_cents: fee,
        amount_cents: amount,
        cost_per_unit,
        date,
    })
}

pub(crate) fn plan_sell(
    rng: &mut Rng,
    pf: &Portfolio,
    md: &MarketData,
    window: Window,
) -> Option<SellTrade> {
    let available: Vec<(usize, f64)> = pf
        .pairs
        .iter()
        .enumerate()
        .map(|(pair, lots)| (pair, lots.iter().map(|l| l.remaining).sum()))
        .filter(|(_, avail)| *avail > 0.0)
        .collect();
    if available.is_empty() {
        return None;
    }
    let &(pair, avail) = rng.pick(&available);
    let slot = pair / INSTRUMENT_TOTAL;
    let inst = pair % INSTRUMENT_TOTAL;

    let mut candidates: Vec<usize> = pf.pairs[pair]
        .iter()
        .enumerate()
        .filter(|(_, lot)| lot.remaining > 0.0)
        .map(|(pos, _)| pos)
        .collect();
    candidates.sort_by(|&a, &b| {
        let (la, lb) = (&pf.pairs[pair][a], &pf.pairs[pair][b]);
        (&la.created, &la.id).cmp(&(&lb.created, &lb.id))
    });
    let full = rng.chance(FULL_SELL_RATE);
    let fund = is_fund(INSTRUMENTS[inst].itype);
    let quantity = if full {
        avail
    } else {
        let frac = PARTIAL_SELL_MIN + rng.next_f64() * PARTIAL_SELL_SPAN;
        if fund {
            ((avail * frac) * 100.0).floor() / 100.0
        } else {
            let hands = ((avail * frac) / 100.0).floor() as i64;
            if hands >= 1 {
                (hands * 100) as f64
            } else {
                avail
            }
        }
    };
    if quantity <= 0.0 {
        return None;
    }

    let mut matches = Vec::new();
    let mut left = quantity;
    let mut matched_max_buy_date = window.start;
    for pos in &candidates {
        if left <= 0.0 {
            break;
        }
        let lot = &pf.pairs[pair][*pos];
        let exhausts = left >= lot.remaining;
        let matched = if exhausts { lot.remaining } else { left };
        if lot.buy_date > matched_max_buy_date {
            matched_max_buy_date = lot.buy_date;
        }
        matches.push(LotMatch {
            pos: *pos,
            quantity: matched,
            cost_per_unit: lot.cost_per_unit,
            initial: lot.initial,
            exhausts,
        });
        left -= matched;
    }
    let date = window.rand_date(rng).max(matched_max_buy_date);
    let price = md.price_at(inst, date);

    let gross = (quantity * price as f64 / PRICE_UNITS_PER_FEN).round() as i64;
    let fee = ((gross as f64 * STOCK_FEE_RATE).round() as i64).min(gross);
    let amount = gross - fee;

    Some(SellTrade {
        slot,
        instrument_idx: inst,
        ccy: INSTRUMENTS[inst].ccy,
        quantity,
        price_units: price,
        fee_cents: fee,
        amount_cents: amount,
        gross_cents: gross,
        date,
        matches,
    })
}

pub(crate) struct SideEffects<'a> {
    pub instrument_id: &'a str,
    pub account_id: &'a str,
    pub created: &'a str,
    pub millis: i64,
}

fn apply_buy(
    conn: &Connection,
    txn_id: &str,
    trade: &BuyTrade,
    fx: &SideEffects,
    pf: &mut Portfolio,
) -> Result<(), String> {
    let created = fx.created;
    let millis = fx.millis;
    let instrument_id = fx.instrument_id;
    let account_id = fx.account_id;
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,\
         price_cents,fee_cents) VALUES (?1,?2,'buy',?3,?4,?5)",
        rusqlite::params![
            txn_id,
            instrument_id,
            trade.quantity,
            trade.price_units,
            trade.fee_cents,
        ],
    )
    .map_err(|e| e.to_string())?;
    let lot_id = time_ordered_id("security_lots", pf.next_lot_seq, millis);
    pf.next_lot_seq += 1;
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,\
         initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,\
         created_at,updated_at,version,device_id)\
         VALUES (?1,?2,?3,?4,?5,?5,?6,?7,?8,?8,1,?9)",
        rusqlite::params![
            lot_id,
            account_id,
            instrument_id,
            txn_id,
            trade.quantity,
            trade.cost_per_unit,
            trade.ccy,
            created,
            DEVICE_ID,
        ],
    )
    .map_err(|e| e.to_string())?;
    let pair = trade.slot * INSTRUMENT_TOTAL + trade.instrument_idx;
    pf.pairs[pair].push(LotState {
        id: lot_id,
        created: created.to_string(),
        buy_date: trade.date,
        initial: trade.quantity,
        remaining: trade.quantity,
        cost_per_unit: trade.cost_per_unit,
        cost_allocated: 0,
    });
    pf.buys += 1;
    Ok(())
}

fn apply_sell(
    conn: &Connection,
    txn_id: &str,
    trade: &SellTrade,
    fx: &SideEffects,
    pf: &mut Portfolio,
) -> Result<(), String> {
    let created = fx.created;
    let millis = fx.millis;
    let instrument_id = fx.instrument_id;
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,\
         price_cents,fee_cents) VALUES (?1,?2,'sell',?3,?4,?5)",
        rusqlite::params![
            txn_id,
            instrument_id,
            trade.quantity,
            trade.price_units,
            trade.fee_cents,
        ],
    )
    .map_err(|e| e.to_string())?;

    let pair = trade.slot * INSTRUMENT_TOTAL + trade.instrument_idx;
    let count = trade.matches.len();
    let mut proceeds_total = 0i64;
    let mut fee_total = 0i64;
    for (i, m) in trade.matches.iter().enumerate() {
        let last = i == count - 1;
        let proceeds = if last {
            trade.gross_cents - proceeds_total
        } else {
            let p = (m.quantity * trade.price_units as f64 / PRICE_UNITS_PER_FEN).round() as i64;
            proceeds_total += p;
            p
        };
        let fee_alloc = if last {
            trade.fee_cents - fee_total
        } else {
            let f = (trade.fee_cents as f64 * m.quantity / trade.quantity).floor() as i64;
            fee_total += f;
            f
        };
        let lot_cost = {
            let lot = &pf.pairs[pair][m.pos];
            if m.exhausts {
                (m.initial * m.cost_per_unit as f64 / PRICE_UNITS_PER_FEN).round() as i64
                    - lot.cost_allocated
            } else {
                (m.quantity * m.cost_per_unit as f64 / PRICE_UNITS_PER_FEN).round() as i64
            }
        };
        let realized = proceeds - lot_cost - fee_alloc;
        let sale_id = time_ordered_id("lot_sales", pf.next_sale_seq, millis);
        pf.next_sale_seq += 1;
        conn.execute(
            "INSERT INTO security_lot_sales (id,sell_transaction_id,lot_id,quantity,\
             cost_per_unit_cents,realized_pnl_cents,currency_code,created_at)\
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![
                sale_id,
                txn_id,
                pf.pairs[pair][m.pos].id,
                m.quantity,
                m.cost_per_unit,
                realized,
                trade.ccy,
                created,
            ],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity-?1,updated_at=?2,\
             version=version+1,device_id=?3 WHERE id=?4",
            rusqlite::params![m.quantity, created, DEVICE_ID, pf.pairs[pair][m.pos].id],
        )
        .map_err(|e| e.to_string())?;
        let lot = &mut pf.pairs[pair][m.pos];
        lot.remaining -= m.quantity;
        if !m.exhausts {
            lot.cost_allocated += lot_cost;
        }
    }
    pf.sells += 1;
    Ok(())
}

pub(crate) fn apply_trade(
    conn: &Connection,
    txn_id: &str,
    trade: TradeKind,
    fx: &SideEffects,
    pf: &mut Portfolio,
) -> Result<(), String> {
    match trade {
        TradeKind::Buy(t) => apply_buy(conn, txn_id, &t, fx, pf),
        TradeKind::Sell(t) => apply_sell(conn, txn_id, &t, fx, pf),
    }
}

pub(crate) enum TradeKind {
    Buy(BuyTrade),
    Sell(Box<SellTrade>),
}

impl TradeKind {
    pub fn row_amount(&self) -> i64 {
        match self {
            TradeKind::Buy(t) => t.amount_cents,
            TradeKind::Sell(t) => t.amount_cents,
        }
    }

    pub fn row_date(&self) -> NaiveDate {
        match self {
            TradeKind::Buy(t) => t.date,
            TradeKind::Sell(t) => t.date,
        }
    }

    pub fn account_slot(&self) -> usize {
        match self {
            TradeKind::Buy(t) => t.slot,
            TradeKind::Sell(t) => t.slot,
        }
    }

    pub fn ccy(&self) -> &'static str {
        match self {
            TradeKind::Buy(t) => t.ccy,
            TradeKind::Sell(t) => t.ccy,
        }
    }

    pub fn instrument_idx(&self) -> usize {
        match self {
            TradeKind::Buy(t) => t.instrument_idx,
            TradeKind::Sell(t) => t.instrument_idx,
        }
    }
}
