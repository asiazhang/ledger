use rusqlite::{Connection, params};

use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;
use tauri_app_lib::test_support::FIXED_NOW;

pub(crate) use crate::{Instrument, InstrumentListFilter, InstrumentListResult};
pub(crate) use ledger_infra::error::AppError;

pub(super) fn insert_instrument_with_market(
    conn: &Connection,
    id: &str,
    symbol: &str,
    name: &str,
    currency: &str,
    market: &str,
    kind: &str,
) {
    try_insert_instrument_with_market(conn, id, symbol, name, currency, market, kind).unwrap();
}

pub(super) fn instrument_by_id(conn: &Connection, id: &str) -> Instrument {
    crate::list_instruments(conn, &InstrumentListFilter::default())
        .unwrap()
        .items
        .into_iter()
        .find(|inst| inst.id == id)
        .unwrap_or_else(|| panic!("标的 {id} 不在列表投影中"))
}

pub(super) fn try_insert_instrument_with_market(
    conn: &Connection,
    id: &str,
    symbol: &str,
    name: &str,
    currency: &str,
    market: &str,
    kind: &str,
) -> rusqlite::Result<String> {
    conn.execute(
         "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
          VALUES (?1,?2,?6,?3,?4,?5,?7,?7,1,'test')",
        params![id, symbol, name, currency, market, kind, FIXED_NOW],
    )?;
    Ok(id.to_string())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn insert_instrument_with_source(
    conn: &Connection,
    id: &str,
    symbol: &str,
    name: &str,
    currency: &str,
    market: &str,
    kind: &str,
    source: &str,
) {
    conn.execute(
         "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id,source) \
          VALUES (?1,?2,?6,?3,?4,?5,?8,?8,1,'test',?7)",
        params![id, symbol, name, currency, market, kind, source, FIXED_NOW],
    )
    .unwrap();
}

pub(super) fn insert_fund_instrument(conn: &Connection, id: &str, symbol: &str, name: &str) {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'fund',?3,'CNY','unknown',?4,?4,1,'test')",
        params![id, symbol, name, FIXED_NOW],
    )
    .unwrap();
}

pub(super) fn make_buy_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: Some(account_id.into()),
        funding: Vec::new(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(fee),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

pub(super) fn make_sell_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Sell,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: Some(account_id.into()),
        funding: Vec::new(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: "2026-01-20".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(fee),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn make_convert_input(
    account_id: &str,
    instrument_id: &str,
    to_instrument_id: &str,
    quantity: f64,
    to_quantity: f64,
    out_amount_cents: i64,
    in_amount_cents: i64,
    fee_cents: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Convert,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: Some(account_id.into()),
        funding: Vec::new(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: "2026-02-01".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(quantity),
        price_cents: None,
        fee_cents: Some(fee_cents),
        to_instrument_id: Some(to_instrument_id.into()),
        to_quantity: Some(to_quantity),
        out_amount_cents: Some(out_amount_cents),
        in_amount_cents: Some(in_amount_cents),
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

pub(super) fn make_split_input(
    account_id: &str,
    instrument_id: &str,
    delta: f64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Split,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: Some(account_id.into()),
        funding: Vec::new(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: "2026-02-01".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(delta),
        price_cents: None,
        fee_cents: Some(0),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

pub(super) fn make_dividend_input(
    account_id: &str,
    instrument_id: &str,
    amount_cents: i64,
    currency: &str,
) -> TransactionInput {
    make_dividend_input_on(
        account_id,
        instrument_id,
        amount_cents,
        currency,
        "2026-02-10",
    )
}

pub(super) fn make_dividend_input_on(
    account_id: &str,
    instrument_id: &str,
    amount_cents: i64,
    currency: &str,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Dividend,
        amount_cents,
        currency_code: currency.into(),
        account_id: Some(account_id.into()),
        funding: Vec::new(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: date.into(),
        instrument_id: Some(instrument_id.into()),
        quantity: None,
        price_cents: None,
        fee_cents: Some(0),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

pub(super) fn make_trade_input(
    kind: TransactionKind,
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: Some(account_id.into()),
        funding: Vec::new(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: date.into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(0),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}
