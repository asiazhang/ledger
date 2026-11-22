use rusqlite::{Connection, OptionalExtension};

use super::guards::{self, InstrumentUse, InvestmentAccountUse};
use super::lots::{self, Consumption};
use super::model::{TransactionConvert, TransactionSplit, TransactionTrade};
use super::prices::PRICE_UNITS_PER_FEN;
use super::split::{self, LotRestatement};
use super::unwind;
use ledger_accounts::AccountType;
use ledger_infra::db::query::query_one;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;
use ledger_transaction::amount::{self, FxEditBaseline, TransactionKind};
use ledger_transaction::command::{
    ConvertCommandFields, InvestmentCommandFields, SplitCommandFields,
};
use ledger_transaction::{ConvertFields, NormalizedTransaction, SecurityOrigin, TransactionInput};

pub(crate) fn require_account(account_id: &Option<String>) -> Result<&str> {
    account_id
        .as_deref()
        .ok_or_else(|| AppError::coded("transaction.account-required", "必须指定账户"))
}

fn account_currency_code(conn: &Connection, account_id: &str) -> Result<String> {
    conn.query_row(
        "SELECT currency_code FROM accounts WHERE id=?1",
        rusqlite::params![account_id],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

fn fetch_instrument_type(
    conn: &Connection,
    instrument_id: &str,
    use_case: InstrumentUse,
) -> Result<String> {
    let instrument_type: Option<String> = conn
        .query_row(
            "SELECT instrument_type FROM instruments WHERE id=?1",
            rusqlite::params![instrument_id],
            |r| r.get(0),
        )
        .optional()?;
    guards::require_instrument_type(instrument_type, instrument_id, use_case)
}

pub fn get_transaction_trade(conn: &Connection, transaction_id: &str) -> Result<TransactionTrade> {
    query_one::<TransactionTrade, _>(
        conn,
        "SELECT st.instrument_id, i.symbol, i.name, i.instrument_type, st.quantity, st.price_cents, st.fee_cents \
         FROM security_transactions st \
         JOIN instruments i ON i.id = st.instrument_id \
         WHERE st.transaction_id = ?1",
        rusqlite::params![transaction_id],
    )?
    .ok_or_else(|| {
        AppError::codedp_not_found(
            "trade.detail-not-found",
            format!("交易不存在或无买卖明细: {transaction_id}"),
            &[transaction_id],
        )
    })
}

pub fn get_transaction_convert(
    conn: &Connection,
    transaction_id: &str,
) -> Result<TransactionConvert> {
    query_one::<TransactionConvert, _>(
        conn,
        "SELECT st.instrument_id, oi.symbol, oi.name, st.quantity, st.out_amount_cents, \
         st.to_instrument_id, ii.symbol, ii.name, st.to_quantity, st.in_amount_cents, \
         st.fee_cents, t.amount_cents, t.currency_code \
         FROM security_transactions st \
         JOIN instruments oi ON oi.id = st.instrument_id \
         JOIN instruments ii ON ii.id = st.to_instrument_id \
         JOIN transactions t ON t.id = st.transaction_id \
         WHERE st.transaction_id = ?1 AND st.action='convert'",
        rusqlite::params![transaction_id],
    )?
    .ok_or_else(|| {
        AppError::codedp_not_found(
            "trade.convert-detail-not-found",
            format!("交易不存在或无转换明细: {transaction_id}"),
            &[transaction_id],
        )
    })
}

pub fn get_transaction_split(conn: &Connection, transaction_id: &str) -> Result<TransactionSplit> {
    query_one::<TransactionSplit, _>(
        conn,
        "SELECT st.instrument_id, i.symbol, i.name, st.quantity \
         FROM security_transactions st \
         JOIN instruments i ON i.id = st.instrument_id \
         WHERE st.transaction_id = ?1 AND st.action='split'",
        rusqlite::params![transaction_id],
    )?
    .ok_or_else(|| {
        AppError::codedp_not_found(
            "trade.split-detail-not-found",
            format!("交易不存在或无份额调整明细: {transaction_id}"),
            &[transaction_id],
        )
    })
}

pub fn convert_fields_by_transaction_ids(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<Vec<(String, ConvertFields)>> {
    if transaction_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; transaction_ids.len()].join(",");
    let mut stmt = conn.prepare(&format!(
        "SELECT st.transaction_id,st.to_instrument_id,i.symbol,st.to_quantity,st.out_amount_cents,st.in_amount_cents \
         FROM security_transactions st \
         JOIN instruments i ON i.id = st.to_instrument_id \
         WHERE st.action='convert' AND st.transaction_id IN ({placeholders})"
    ))?;
    let rows = stmt.query_map(rusqlite::params_from_iter(transaction_ids.iter()), |r| {
        Ok((
            r.get::<_, String>(0)?,
            ConvertFields {
                to_instrument_id: r.get(1)?,
                to_symbol: r.get(2)?,
                to_quantity: r.get(3)?,
                out_amount_cents: r.get(4)?,
                in_amount_cents: r.get(5)?,
            },
        ))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub struct BuyPlan {
    pub(crate) normalized: NormalizedTransaction,
    pub(crate) instrument_id: String,
    pub(crate) quantity: f64,
    pub(crate) price_cents: i64,
    pub(crate) fee_cents: i64,
    pub(crate) cost_per_unit_cents: i64,
    pub(crate) origin: SecurityOrigin,
}

fn prepare_buy(
    conn: &Connection,
    input: &TransactionInput,
    fx_edit_baseline: Option<&FxEditBaseline>,
) -> Result<BuyPlan> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| AppError::coded("trade.buy-instrument-required", "买入必须指定标的"))?
        .clone();
    let instrument_type = fetch_instrument_type(conn, &instrument_id, InstrumentUse::Buy)?;
    guards::reject_to_account(TransactionKind::Buy, input.to_account_id.is_some())?;

    let quantity = input.quantity.unwrap_or(0.0);
    let fee_cents = input.fee_cents.unwrap_or(0);
    guards::require_positive(guards::PositiveValue::BuyQuantity, quantity)?;
    let is_fund = instrument_type == "fund";
    let price_cents;
    let amount_cents;
    let cost_per_unit_cents;
    if is_fund {
        amount_cents = input.amount_cents;
        if amount_cents <= 0 {
            return Err(AppError::coded(
                "trade.buy-amount-positive",
                "买入金额必须大于 0（基金申赎以确认单金额为权威）",
            ));
        }
        if input.price_cents.is_some() {
            return Err(AppError::coded(
                "trade.fund-price-forbidden",
                "基金申赎以确认单金额为权威，不可提供单价（由金额与份额反算）",
            ));
        }
        if fee_cents >= amount_cents {
            return Err(AppError::coded(
                "trade.buy-fee-exceeds-amount",
                "买入手续费不能超过买入金额",
            ));
        }
        price_cents =
            ((amount_cents - fee_cents) as f64 * PRICE_UNITS_PER_FEN / quantity).round() as i64;
        if price_cents <= 0 {
            return Err(AppError::coded(
                "trade.derived-price-positive",
                "反算单价必须大于 0（确认金额过小或份额过大）",
            ));
        }
        cost_per_unit_cents = (amount_cents as f64 * PRICE_UNITS_PER_FEN / quantity).round() as i64;
    } else {
        price_cents = input.price_cents.unwrap_or(0);
        if price_cents <= 0 {
            return Err(AppError::coded(
                "trade.buy-price-positive",
                "买入单价必须大于 0",
            ));
        }
        amount_cents =
            (quantity * price_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64 + fee_cents;
        cost_per_unit_cents =
            ((quantity * price_cents as f64 + fee_cents as f64 * PRICE_UNITS_PER_FEN) / quantity)
                .round() as i64;
    }
    let account_id = require_account(&input.account_id)?;
    ensure_investment_account(conn, account_id, InvestmentAccountUse::Buy)?;
    let account_currency = account_currency_code(conn, account_id)?;
    ledger_transaction::write::funding::validate_funding_account(
        conn,
        TransactionKind::Buy,
        input.funding_account_id.as_deref(),
        &account_currency,
    )?;
    let native = amount::convert_to_native_on_edit(
        conn,
        amount_cents,
        &account_currency,
        &input.date,
        input.fx_rate,
        fx_edit_baseline,
    )?;

    Ok(BuyPlan {
        normalized: NormalizedTransaction {
            kind: TransactionKind::Buy,
            amount_cents,
            currency_code: account_currency,
            amount_native_cents: native.native_cents,
            fx_rate_used: native.fx_rate_used,
            fx_rate_source: native.fx_rate_source,
            account_id: input.account_id.clone(),
            funding: Vec::new(),
            purchases: Vec::new(),
            to_account_id: None,
            funding_account_id: input.funding_account_id.clone(),
            category_id: None,
            merchant_id: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            source_order_no: input.source_order_no.clone(),
            date: input.date.clone(),
        },
        instrument_id,
        quantity,
        price_cents,
        fee_cents,
        cost_per_unit_cents,
        origin: input.origin.unwrap_or_default(),
    })
}

fn prepare_sell(
    conn: &Connection,
    input: &TransactionInput,
    fx_edit_baseline: Option<&FxEditBaseline>,
) -> Result<SellPlan> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| AppError::coded("trade.sell-instrument-required", "卖出必须指定标的"))?
        .clone();
    let instrument_type = fetch_instrument_type(conn, &instrument_id, InstrumentUse::Sell)?;
    guards::reject_to_account(TransactionKind::Sell, input.to_account_id.is_some())?;

    let quantity = input.quantity.unwrap_or(0.0);
    let fee_cents = input.fee_cents.unwrap_or(0);
    guards::require_positive(guards::PositiveValue::SellQuantity, quantity)?;
    let is_fund = instrument_type == "fund";
    let price_cents;
    let amount_cents;
    let gross_proceeds;
    if is_fund {
        amount_cents = input.amount_cents;
        if amount_cents <= 0 {
            return Err(AppError::coded(
                "trade.sell-amount-positive",
                "卖出金额必须大于 0（基金申赎以确认单金额为权威）",
            ));
        }
        if input.price_cents.is_some() {
            return Err(AppError::coded(
                "trade.fund-price-forbidden",
                "基金申赎以确认单金额为权威，不可提供单价（由金额与份额反算）",
            ));
        }
        gross_proceeds = amount_cents + fee_cents;
        price_cents = (gross_proceeds as f64 * PRICE_UNITS_PER_FEN / quantity).round() as i64;
        if price_cents <= 0 {
            return Err(AppError::coded(
                "trade.derived-price-positive",
                "反算单价必须大于 0（确认金额过小或份额过大）",
            ));
        }
    } else {
        price_cents = input.price_cents.unwrap_or(0);
        if price_cents <= 0 {
            return Err(AppError::coded(
                "trade.sell-price-positive",
                "卖出单价必须大于 0",
            ));
        }
        gross_proceeds = (quantity * price_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64;
        if fee_cents > gross_proceeds {
            return Err(AppError::coded(
                "trade.sell-fee-exceeds-proceeds",
                "卖出手续费不能超过卖出收入",
            ));
        }
        amount_cents = gross_proceeds - fee_cents;
    }
    let account_id = require_account(&input.account_id)?;
    ensure_investment_account(conn, account_id, InvestmentAccountUse::Sell)?;
    let account_currency = account_currency_code(conn, account_id)?;
    ledger_transaction::write::funding::validate_funding_account(
        conn,
        TransactionKind::Sell,
        input.funding_account_id.as_deref(),
        &account_currency,
    )?;
    let native = amount::convert_to_native_on_edit(
        conn,
        amount_cents,
        &account_currency,
        &input.date,
        input.fx_rate,
        fx_edit_baseline,
    )?;

    let active_lots = lots::active_lots(conn, account_id, &instrument_id)?;
    let consumed = lots::plan(conn, &active_lots, quantity)?;

    Ok(SellPlan {
        normalized: NormalizedTransaction {
            kind: TransactionKind::Sell,
            amount_cents,
            currency_code: account_currency,
            amount_native_cents: native.native_cents,
            fx_rate_used: native.fx_rate_used,
            fx_rate_source: native.fx_rate_source,
            account_id: input.account_id.clone(),
            funding: Vec::new(),
            purchases: Vec::new(),
            to_account_id: None,
            funding_account_id: input.funding_account_id.clone(),
            category_id: None,
            merchant_id: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            source_order_no: input.source_order_no.clone(),
            date: input.date.clone(),
        },
        instrument_id,
        quantity,
        price_cents,
        fee_cents,
        consumed,
        gross_proceeds_cents: gross_proceeds,
    })
}

fn derived_out_price_cents(out_amount_cents: i64, quantity: f64) -> Result<i64> {
    let price_cents = (out_amount_cents as f64 * PRICE_UNITS_PER_FEN / quantity).round() as i64;
    if price_cents <= 0 {
        return Err(AppError::coded(
            "trade.derived-price-positive",
            "反算单价必须大于 0（确认金额过小或份额过大）",
        ));
    }
    Ok(price_cents)
}

fn prepare_convert(
    conn: &Connection,
    input: &TransactionInput,
    fx_edit_baseline: Option<&FxEditBaseline>,
) -> Result<ConvertPlan> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| {
            AppError::coded("trade.convert-instrument-required", "转换必须指定转出标的")
        })?
        .clone();
    let to_instrument_id = input
        .to_instrument_id
        .as_ref()
        .ok_or_else(|| {
            AppError::coded(
                "trade.convert-to-instrument-required",
                "转换必须指定转入标的",
            )
        })?
        .clone();
    guards::reject_same_instrument(instrument_id == to_instrument_id)?;
    fetch_instrument_type(conn, &instrument_id, InstrumentUse::ConvertOut)?;
    fetch_instrument_type(conn, &to_instrument_id, InstrumentUse::ConvertIn)?;
    let quantity = input.quantity.unwrap_or(0.0);
    guards::require_positive(guards::PositiveValue::ConvertQuantity, quantity)?;
    let to_quantity = input.to_quantity.unwrap_or(0.0);
    guards::require_positive(guards::PositiveValue::ConvertToQuantity, to_quantity)?;
    let fee_cents = input.fee_cents.unwrap_or(0);
    let out_amount_cents = input.out_amount_cents.unwrap_or(0);
    guards::require_positive_cents(guards::PositiveValue::ConvertOutAmount, out_amount_cents)?;
    let in_amount_cents = input.in_amount_cents.unwrap_or(0);
    guards::require_positive_cents(guards::PositiveValue::ConvertInAmount, in_amount_cents)?;
    let price_cents = derived_out_price_cents(out_amount_cents, quantity)?;
    let account_id = require_account(&input.account_id)?;
    ensure_investment_account(conn, account_id, InvestmentAccountUse::Convert)?;
    guards::reject_to_account(TransactionKind::Convert, input.to_account_id.is_some())?;

    let account_currency = account_currency_code(conn, account_id)?;
    ledger_transaction::write::funding::validate_funding_account(
        conn,
        TransactionKind::Convert,
        input.funding_account_id.as_deref(),
        &account_currency,
    )?;
    let active_lots = lots::active_lots(conn, account_id, &instrument_id)?;
    let consumed = lots::plan(conn, &active_lots, quantity)?;
    let carried_cost_cents = lots::total_cost(&consumed);
    let native = amount::convert_to_native_on_edit(
        conn,
        carried_cost_cents,
        &account_currency,
        &input.date,
        input.fx_rate,
        fx_edit_baseline,
    )?;

    Ok(ConvertPlan {
        normalized: NormalizedTransaction {
            kind: TransactionKind::Convert,
            amount_cents: carried_cost_cents,
            currency_code: account_currency,
            amount_native_cents: native.native_cents,
            fx_rate_used: native.fx_rate_used,
            fx_rate_source: native.fx_rate_source,
            account_id: input.account_id.clone(),
            funding: Vec::new(),
            purchases: Vec::new(),
            to_account_id: None,
            funding_account_id: None,
            category_id: None,
            merchant_id: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            source_order_no: input.source_order_no.clone(),
            date: input.date.clone(),
        },
        instrument_id,
        to_instrument_id,
        quantity,
        to_quantity,
        price_cents,
        fee_cents,
        out_amount_cents,
        in_amount_cents,
        consumed,
    })
}

pub struct SplitPlan {
    pub(crate) normalized: NormalizedTransaction,
    pub(crate) instrument_id: String,
    pub(crate) delta_quantity: f64,
    pub(crate) restated: Vec<LotRestatement>,
    pub(crate) final_quantity: f64,
    pub(crate) total_cost_cents: i64,
}

pub struct DividendPlan {
    pub(crate) normalized: NormalizedTransaction,
    pub(crate) instrument_id: String,
}

fn prepare_split(
    conn: &Connection,
    input: &TransactionInput,
    existing_id: Option<&str>,
) -> Result<SplitPlan> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| AppError::coded("trade.split-instrument-required", "份额调整必须指定标的"))?
        .clone();
    fetch_instrument_type(conn, &instrument_id, InstrumentUse::Split)?;
    if input.to_instrument_id.is_some() {
        return Err(AppError::coded(
            "trade.split-to-instrument-forbidden",
            "份额调整是单标的份额变动，不能携带转入标的",
        ));
    }
    guards::reject_to_account(TransactionKind::Split, input.to_account_id.is_some())?;

    if input.price_cents.is_some() {
        return Err(AppError::coded(
            "trade.split-price-forbidden",
            "份额调整无现金腿，不可提供单价",
        ));
    }
    let fee_cents = input.fee_cents.unwrap_or(0);
    if fee_cents != 0 {
        return Err(AppError::coded(
            "trade.split-fee-forbidden",
            "份额调整无现金腿，不接受手续费",
        ));
    }
    guards::reject_split_cash_leg(input.amount_cents != 0)?;
    if input.fx_rate.is_some() {
        return Err(AppError::coded(
            "trade.split-fx-rate-forbidden",
            "份额调整无现金腿、不折算，不能携带显式汇率",
        ));
    }
    let delta_quantity = input.quantity.unwrap_or(0.0);
    guards::require_nonzero_split_quantity(delta_quantity)?;
    let account_id = require_account(&input.account_id)?;
    ensure_investment_account(conn, account_id, InvestmentAccountUse::Split)?;
    let account_currency = account_currency_code(conn, account_id)?;
    ledger_transaction::write::funding::validate_funding_account(
        conn,
        TransactionKind::Split,
        input.funding_account_id.as_deref(),
        &account_currency,
    )?;
    let before_rowid = match existing_id {
        Some(id) => Some(conn.query_row(
            "SELECT rowid FROM transactions WHERE id=?1",
            rusqlite::params![id],
            |r| r.get::<_, i64>(0),
        )?),
        None => None,
    };
    let restatement = split::plan_restatement(
        conn,
        account_id,
        &instrument_id,
        delta_quantity,
        before_rowid,
    )?;
    guards::require_split_holding(!restatement.lots.is_empty())?;
    let zero_leg = amount::NativeConversion::zero_cash_leg();

    Ok(SplitPlan {
        normalized: NormalizedTransaction {
            kind: TransactionKind::Split,
            amount_cents: 0,
            currency_code: account_currency,
            amount_native_cents: zero_leg.native_cents,
            fx_rate_used: zero_leg.fx_rate_used,
            fx_rate_source: zero_leg.fx_rate_source,
            account_id: input.account_id.clone(),
            funding: Vec::new(),
            purchases: Vec::new(),
            to_account_id: None,
            funding_account_id: None,
            category_id: None,
            merchant_id: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            source_order_no: input.source_order_no.clone(),
            date: input.date.clone(),
        },
        instrument_id,
        delta_quantity,
        restated: restatement.lots,
        final_quantity: restatement.final_quantity,
        total_cost_cents: restatement.total_cost_cents,
    })
}

fn prepare_dividend(
    conn: &Connection,
    input: &TransactionInput,
    fx_edit_baseline: Option<&FxEditBaseline>,
) -> Result<DividendPlan> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| AppError::coded("trade.dividend-instrument-required", "分红必须指定标的"))?
        .clone();
    fetch_instrument_type(conn, &instrument_id, InstrumentUse::Dividend)?;
    if input.to_instrument_id.is_some() {
        return Err(AppError::coded(
            "trade.dividend-to-instrument-forbidden",
            "分红是单标的现金收入，不能携带转入标的",
        ));
    }
    guards::reject_to_account(TransactionKind::Dividend, input.to_account_id.is_some())?;

    if input.quantity.is_some() {
        return Err(AppError::coded(
            "trade.dividend-quantity-forbidden",
            "分红无份额变动，不可提供数量",
        ));
    }
    if input.price_cents.is_some() {
        return Err(AppError::coded(
            "trade.dividend-price-forbidden",
            "分红无成交单价，不可提供单价",
        ));
    }
    if input.fee_cents.unwrap_or(0) != 0 {
        return Err(AppError::coded(
            "trade.dividend-fee-forbidden",
            "分红不接受手续费",
        ));
    }
    guards::require_positive_cents(guards::PositiveValue::DividendAmount, input.amount_cents)?;
    let account_currency = active_account_currency(conn, require_account(&input.account_id)?)?;
    guards::require_currency_match(&input.currency_code, &account_currency)?;
    ledger_transaction::write::funding::validate_funding_account(
        conn,
        TransactionKind::Dividend,
        input.funding_account_id.as_deref(),
        &account_currency,
    )?;
    let native = amount::convert_to_native_on_edit(
        conn,
        input.amount_cents,
        &account_currency,
        &input.date,
        input.fx_rate,
        fx_edit_baseline,
    )?;
    Ok(DividendPlan {
        normalized: NormalizedTransaction {
            kind: TransactionKind::Dividend,
            amount_cents: input.amount_cents,
            currency_code: account_currency,
            amount_native_cents: native.native_cents,
            fx_rate_used: native.fx_rate_used,
            fx_rate_source: native.fx_rate_source,
            account_id: input.account_id.clone(),
            funding: Vec::new(),
            purchases: Vec::new(),
            to_account_id: None,
            funding_account_id: None,
            category_id: None,
            merchant_id: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            source_order_no: input.source_order_no.clone(),
            date: input.date.clone(),
        },
        instrument_id,
    })
}

fn active_account_currency(conn: &Connection, account_id: &str) -> Result<String> {
    conn.query_row(
        "SELECT currency_code FROM accounts WHERE id=?1 AND is_deleted=0",
        rusqlite::params![account_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| {
        AppError::codedp_not_found(
            "trade.dividend-account-not-found",
            format!("分红到账账户不存在或已删除: {account_id}"),
            &[account_id],
        )
    })
}

fn write_dividend_side_effects(conn: &Connection, id: &str, instrument_id: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'dividend',NULL,NULL,0)",
        rusqlite::params![id, instrument_id],
    )?;
    Ok(())
}

fn write_sell_side_effects(conn: &Connection, id: &str, plan: &SellPlan) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'sell',?3,?4,?5)",
        rusqlite::params![id, plan.instrument_id, plan.quantity, plan.price_cents, plan.fee_cents],
    )?;

    let consumptions = &plan.consumed;

    let match_count = consumptions.len();
    let mut allocated_fee_total = 0i64;
    let mut allocated_proceeds_total = 0i64;
    for (i, matched) in consumptions.iter().enumerate() {
        let lot = &matched.lot;
        let lot_proceeds = if i == match_count - 1 {
            plan.gross_proceeds_cents - allocated_proceeds_total
        } else {
            let proceeds = (lot.remaining_quantity * plan.price_cents as f64 / PRICE_UNITS_PER_FEN)
                .round() as i64;
            allocated_proceeds_total += proceeds;
            proceeds
        };
        let lot_cost = matched.cost_cents;
        let allocated_fee = if i == match_count - 1 {
            plan.fee_cents - allocated_fee_total
        } else {
            let fee =
                (plan.fee_cents as f64 * lot.remaining_quantity / plan.quantity).floor() as i64;
            allocated_fee_total += fee;
            fee
        };
        let realized_pnl = lot_proceeds - lot_cost - allocated_fee;
        let sale_id = new_uuid();
        conn.execute(
            "INSERT INTO security_lot_sales (id,sell_transaction_id,lot_id,quantity,cost_per_unit_cents,realized_pnl_cents,currency_code,created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![sale_id, id, lot.id, lot.remaining_quantity, lot.cost_per_unit_cents, realized_pnl, lot.currency_code, now],
        )?;
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity-?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![lot.remaining_quantity, now, device_id(conn)?, lot.id],
        )?;
    }

    Ok(())
}

pub fn release_for_delete(
    conn: &Connection,
    id: &str,
    kind: TransactionKind,
) -> Result<Vec<String>> {
    unwind::remove(conn, id, kind, unwind::Mode::Delete)
}

fn create_buy_lot(conn: &Connection, transaction_id: &str, plan: &BuyPlan) -> Result<()> {
    let lot_id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents,origin) \
         VALUES (?1,?2,'buy',?3,?4,?5,?6)",
        rusqlite::params![transaction_id, plan.instrument_id, plan.quantity, plan.price_cents, plan.fee_cents, plan.origin.as_db()],
    )?;
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?5,?6,?7,?8,?8,?9,?10)",
        rusqlite::params![lot_id, plan.normalized.account_id, plan.instrument_id, transaction_id, plan.quantity, plan.cost_per_unit_cents, plan.normalized.currency_code, now, 1, device_id(conn)?],
    )?;
    Ok(())
}

pub struct SellPlan {
    pub(crate) normalized: NormalizedTransaction,
    pub(crate) instrument_id: String,
    pub(crate) quantity: f64,
    pub(crate) price_cents: i64,
    pub(crate) fee_cents: i64,
    pub(crate) consumed: Vec<Consumption>,
    pub(crate) gross_proceeds_cents: i64,
}

pub struct ConvertPlan {
    pub(crate) normalized: NormalizedTransaction,
    pub(crate) instrument_id: String,
    pub(crate) to_instrument_id: String,
    pub(crate) quantity: f64,
    pub(crate) to_quantity: f64,
    pub(crate) price_cents: i64,
    pub(crate) fee_cents: i64,
    pub(crate) out_amount_cents: i64,
    pub(crate) in_amount_cents: i64,
    pub(crate) consumed: Vec<Consumption>,
}

impl ConvertPlan {
    pub(crate) fn carried_cost_cents(&self) -> i64 {
        lots::total_cost(&self.consumed)
    }
}

pub enum Plan {
    Buy(BuyPlan),
    Sell(SellPlan),
    Convert(ConvertPlan),
    Split(SplitPlan),
    Dividend(DividendPlan),
}

impl Plan {
    pub fn normalized(&self) -> &NormalizedTransaction {
        match self {
            Plan::Buy(p) => &p.normalized,
            Plan::Sell(p) => &p.normalized,
            Plan::Convert(p) => &p.normalized,
            Plan::Split(p) => &p.normalized,
            Plan::Dividend(p) => &p.normalized,
        }
    }
}

pub fn prepare(
    conn: &Connection,
    kind: TransactionKind,
    input: &TransactionInput,
    existing_id: Option<&str>,
    fx_edit_baseline: Option<&FxEditBaseline>,
) -> Result<Plan> {
    match kind {
        TransactionKind::Buy => Ok(Plan::Buy(prepare_buy(conn, input, fx_edit_baseline)?)),
        TransactionKind::Sell => Ok(Plan::Sell(prepare_sell(conn, input, fx_edit_baseline)?)),
        TransactionKind::Convert => Ok(Plan::Convert(prepare_convert(
            conn,
            input,
            fx_edit_baseline,
        )?)),
        TransactionKind::Split => Ok(Plan::Split(prepare_split(conn, input, existing_id)?)),
        TransactionKind::Dividend => Ok(Plan::Dividend(prepare_dividend(
            conn,
            input,
            fx_edit_baseline,
        )?)),
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => Err(AppError::Invalid(format!(
            "投资层仅处理 buy/sell/convert/split/dividend，收到: {kind}"
        ))),
    }
}

pub fn apply(conn: &Connection, id: &str, plan: &Plan) -> Result<()> {
    match plan {
        Plan::Buy(p) => create_buy_lot(conn, id, p),
        Plan::Sell(p) => write_sell_side_effects(conn, id, p),
        Plan::Convert(p) => write_convert_side_effects(conn, id, p),
        Plan::Split(p) => split::write_split_side_effects(
            conn,
            id,
            &p.instrument_id,
            p.delta_quantity,
            &p.restated,
        ),
        Plan::Dividend(p) => write_dividend_side_effects(conn, id, &p.instrument_id),
    }
}

pub fn revert(conn: &Connection, id: &str, kind: TransactionKind) -> Result<()> {
    unwind::remove(conn, id, kind, unwind::Mode::Update).map(|_| ())
}

fn write_convert_side_effects(conn: &Connection, id: &str, plan: &ConvertPlan) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents,to_instrument_id,to_quantity,out_amount_cents,in_amount_cents) \
         VALUES (?1,?2,'convert',?3,?4,?5,?6,?7,?8,?9)",
        rusqlite::params![
            id,
            plan.instrument_id,
            plan.quantity,
            plan.price_cents,
            plan.fee_cents,
            plan.to_instrument_id,
            plan.to_quantity,
            plan.out_amount_cents,
            plan.in_amount_cents
        ],
    )?;
    for consumed in &plan.consumed {
        conn.execute(
            "INSERT INTO security_lot_conversions (id,transaction_id,lot_id,quantity,cost_per_unit_cents,cost_cents,created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                new_uuid(),
                id,
                consumed.lot.id,
                consumed.lot.remaining_quantity,
                consumed.lot.cost_per_unit_cents,
                consumed.cost_cents,
                now
            ],
        )?;
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity-?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![consumed.lot.remaining_quantity, now, device_id(conn)?, consumed.lot.id],
        )?;
    }
    let carried_cost_cents = plan.carried_cost_cents();
    let cost_per_unit_cents =
        (carried_cost_cents as f64 * PRICE_UNITS_PER_FEN / plan.to_quantity).round() as i64;
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?5,?6,?7,?8,?8,?9,?10)",
        rusqlite::params![
            new_uuid(),
            plan.normalized.account_id,
            plan.to_instrument_id,
            id,
            plan.to_quantity,
            cost_per_unit_cents,
            plan.normalized.currency_code,
            now,
            1,
            device_id(conn)?
        ],
    )?;
    Ok(())
}

pub(crate) fn replay_plan(
    conn: &Connection,
    kind: TransactionKind,
    row: &NormalizedTransaction,
    fields: &InvestmentCommandFields,
) -> Result<Plan> {
    match kind {
        TransactionKind::Buy => {
            guards::reject_to_account(TransactionKind::Buy, row.to_account_id.is_some())?;

            guards::require_positive(guards::PositiveValue::BuyQuantity, fields.quantity)?;
            fetch_instrument_type(conn, &fields.instrument_id, InstrumentUse::Buy)?;
            ensure_investment_account(
                conn,
                require_account(&row.account_id)?,
                InvestmentAccountUse::Buy,
            )?;
            let cost_per_unit_cents = fields
                .cost_per_unit_cents
                .ok_or_else(|| AppError::Invalid("买入命令缺少每份成本字段（程序缺陷）".into()))?;
            Ok(Plan::Buy(BuyPlan {
                normalized: row.clone(),
                instrument_id: fields.instrument_id.clone(),
                quantity: fields.quantity,
                price_cents: fields.price_cents,
                fee_cents: fields.fee_cents,
                cost_per_unit_cents,
                origin: fields.origin.unwrap_or_default(),
            }))
        }
        TransactionKind::Sell => {
            guards::reject_to_account(TransactionKind::Sell, row.to_account_id.is_some())?;

            guards::require_positive(guards::PositiveValue::SellQuantity, fields.quantity)?;
            let instrument_type =
                fetch_instrument_type(conn, &fields.instrument_id, InstrumentUse::Sell)?;
            ensure_investment_account(
                conn,
                require_account(&row.account_id)?,
                InvestmentAccountUse::Sell,
            )?;
            let gross_proceeds_cents = if instrument_type == "fund" {
                row.amount_cents + fields.fee_cents
            } else {
                (fields.quantity * fields.price_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64
            };
            let active_lots = lots::active_lots(
                conn,
                require_account(&row.account_id)?,
                &fields.instrument_id,
            )?;
            let consumed = lots::plan(conn, &active_lots, fields.quantity)?;
            Ok(Plan::Sell(SellPlan {
                normalized: row.clone(),
                instrument_id: fields.instrument_id.clone(),
                quantity: fields.quantity,
                price_cents: fields.price_cents,
                fee_cents: fields.fee_cents,
                consumed,
                gross_proceeds_cents,
            }))
        }
        TransactionKind::Dividend => {
            fetch_instrument_type(conn, &fields.instrument_id, InstrumentUse::Dividend)?;
            guards::require_positive_cents(
                guards::PositiveValue::DividendAmount,
                row.amount_cents,
            )?;
            guards::reject_to_account(TransactionKind::Dividend, row.to_account_id.is_some())?;

            ledger_transaction::write::funding::validate_funding_account(
                conn,
                TransactionKind::Dividend,
                row.funding_account_id.as_deref(),
                &row.currency_code,
            )?;
            let account_currency =
                active_account_currency(conn, require_account(&row.account_id)?)?;
            guards::require_currency_match(&row.currency_code, &account_currency)?;
            Ok(Plan::Dividend(DividendPlan {
                normalized: row.clone(),
                instrument_id: fields.instrument_id.clone(),
            }))
        }
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund
        | TransactionKind::Split
        | TransactionKind::Convert => Err(AppError::Invalid(format!(
            "投资层重放仅处理 buy/sell/dividend（convert/split 走各自入口），收到: {kind}"
        ))),
    }
}

pub(crate) fn replay_convert_plan(
    conn: &Connection,
    row: &NormalizedTransaction,
    fields: &ConvertCommandFields,
) -> Result<Plan> {
    guards::reject_same_instrument(fields.instrument_id == fields.to_instrument_id)?;
    fetch_instrument_type(conn, &fields.instrument_id, InstrumentUse::ConvertOut)?;
    fetch_instrument_type(conn, &fields.to_instrument_id, InstrumentUse::ConvertIn)?;
    guards::require_positive(guards::PositiveValue::ConvertQuantity, fields.quantity)?;
    guards::require_positive(guards::PositiveValue::ConvertToQuantity, fields.to_quantity)?;
    guards::require_positive_cents(
        guards::PositiveValue::ConvertOutAmount,
        fields.out_amount_cents,
    )?;
    guards::require_positive_cents(
        guards::PositiveValue::ConvertInAmount,
        fields.in_amount_cents,
    )?;
    let price_cents = derived_out_price_cents(fields.out_amount_cents, fields.quantity)?;
    ensure_investment_account(
        conn,
        require_account(&row.account_id)?,
        InvestmentAccountUse::Convert,
    )?;
    guards::reject_to_account(TransactionKind::Convert, row.to_account_id.is_some())?;

    ledger_transaction::write::funding::validate_funding_account(
        conn,
        TransactionKind::Convert,
        row.funding_account_id.as_deref(),
        &row.currency_code,
    )?;
    let active_lots = lots::active_lots(
        conn,
        require_account(&row.account_id)?,
        &fields.instrument_id,
    )?;
    let consumed = lots::plan(conn, &active_lots, fields.quantity)?;
    let rebuilt_cost_cents = lots::total_cost(&consumed);
    if rebuilt_cost_cents != fields.carried_cost_cents
        || row.amount_cents != fields.carried_cost_cents
    {
        return Err(AppError::codedp(
            "transaction.convert-carried-cost-mismatch",
            format!(
                "转换结转成本校验不一致（源端 {}，本机重建 {rebuilt_cost_cents}，行金额 {}），已挂起等待处理",
                fields.carried_cost_cents, row.amount_cents
            ),
            &[
                &fields.carried_cost_cents.to_string(),
                &rebuilt_cost_cents.to_string(),
                &row.amount_cents.to_string(),
            ],
        ));
    }
    Ok(Plan::Convert(ConvertPlan {
        normalized: row.clone(),
        instrument_id: fields.instrument_id.clone(),
        to_instrument_id: fields.to_instrument_id.clone(),
        quantity: fields.quantity,
        to_quantity: fields.to_quantity,
        price_cents,
        fee_cents: fields.fee_cents,
        out_amount_cents: fields.out_amount_cents,
        in_amount_cents: fields.in_amount_cents,
        consumed,
    }))
}

pub(crate) fn replay_split_plan(
    conn: &Connection,
    row: &NormalizedTransaction,
    fields: &SplitCommandFields,
    existing_id: Option<&str>,
) -> Result<Plan> {
    fetch_instrument_type(conn, &fields.instrument_id, InstrumentUse::Split)?;
    guards::require_nonzero_split_quantity(fields.delta_quantity)?;
    let zero_leg = amount::NativeConversion::zero_cash_leg();
    guards::reject_split_cash_leg(
        row.amount_cents != 0 || row.amount_native_cents != zero_leg.native_cents,
    )?;
    ensure_investment_account(
        conn,
        require_account(&row.account_id)?,
        InvestmentAccountUse::Split,
    )?;
    guards::reject_to_account(TransactionKind::Split, row.to_account_id.is_some())?;

    ledger_transaction::write::funding::validate_funding_account(
        conn,
        TransactionKind::Split,
        row.funding_account_id.as_deref(),
        &row.currency_code,
    )?;
    let before_rowid = match existing_id {
        Some(id) => Some(conn.query_row(
            "SELECT rowid FROM transactions WHERE id=?1",
            rusqlite::params![id],
            |r| r.get::<_, i64>(0),
        )?),
        None => None,
    };
    let restatement = split::plan_restatement(
        conn,
        require_account(&row.account_id)?,
        &fields.instrument_id,
        fields.delta_quantity,
        before_rowid,
    )?;
    guards::require_split_holding(!restatement.lots.is_empty())?;
    let quantity_diverges =
        (restatement.final_quantity - fields.final_quantity).abs() > lots::QTY_GUARD_EPSILON;
    if quantity_diverges || restatement.total_cost_cents != fields.total_cost_cents {
        let source_quantity = guards::format_quantity_for_message(fields.final_quantity);
        let local_quantity = guards::format_quantity_for_message(restatement.final_quantity);
        let source_cost = fields.total_cost_cents.to_string();
        let local_cost = restatement.total_cost_cents.to_string();
        return Err(AppError::codedp(
            "transaction.split-restatement-mismatch",
            format!(
                "份额调整重放校验不一致（源端 最终持仓 {source_quantity}、总成本 {source_cost}；本机重建 {local_quantity}、{local_cost}），已挂起等待处理"
            ),
            &[&source_quantity, &source_cost, &local_quantity, &local_cost],
        ));
    }
    Ok(Plan::Split(SplitPlan {
        normalized: row.clone(),
        instrument_id: fields.instrument_id.clone(),
        delta_quantity: fields.delta_quantity,
        restated: restatement.lots,
        final_quantity: restatement.final_quantity,
        total_cost_cents: restatement.total_cost_cents,
    }))
}

fn ensure_investment_account(
    conn: &Connection,
    account_id: &str,
    use_case: InvestmentAccountUse,
) -> Result<()> {
    let account_type: AccountType = conn
        .query_row(
            "SELECT type FROM accounts WHERE id=?1",
            rusqlite::params![account_id],
            |r| r.get::<_, String>(0),
        )?
        .parse()?;
    guards::require_investment_account(account_type, use_case)
}
