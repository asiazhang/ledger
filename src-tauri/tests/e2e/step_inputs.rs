use ledger_scheduled::{CreateScheduledInput, RecurrenceType, ScheduledKind};
use ledger_transaction::Transaction;
use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;

fn txn_base(
    kind: TransactionKind,
    amount_cents: i64,
    account_id: &str,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        kind,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: Some(account_id.into()),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        merchant_name: None,
        policy_id: None,
        refund_of_transaction_id: None,
        funding_account_id: None,
        source_order_no: None,
        note: None,
        date: date.into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
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

pub fn expense_input(amount_cents: i64, account_id: &str, date: &str) -> TransactionInput {
    txn_base(TransactionKind::Expense, amount_cents, account_id, date)
}

pub fn income_input(amount_cents: i64, account_id: &str, date: &str) -> TransactionInput {
    txn_base(TransactionKind::Income, amount_cents, account_id, date)
}

pub fn transfer_input(
    amount_cents: i64,
    from_account_id: &str,
    to_account_id: &str,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        to_account_id: Some(to_account_id.into()),
        ..txn_base(
            TransactionKind::Transfer,
            amount_cents,
            from_account_id,
            date,
        )
    }
}

pub fn refund_input(
    amount_cents: i64,
    account_id: &str,
    refund_of_transaction_id: &str,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        refund_of_transaction_id: Some(refund_of_transaction_id.into()),
        ..txn_base(TransactionKind::Refund, amount_cents, account_id, date)
    }
}

pub fn buy_input(
    instrument_id: &str,
    quantity: f64,
    price_cents: Option<i64>,
    account_id: &str,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        instrument_id: Some(instrument_id.into()),
        quantity: Some(quantity),
        price_cents,
        ..txn_base(TransactionKind::Buy, 0, account_id, date)
    }
}

pub fn sell_input(
    instrument_id: &str,
    quantity: f64,
    price_cents: Option<i64>,
    account_id: &str,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        instrument_id: Some(instrument_id.into()),
        quantity: Some(quantity),
        price_cents,
        ..txn_base(TransactionKind::Sell, 0, account_id, date)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn convert_input(
    account_id: &str,
    out_instrument_id: &str,
    out_quantity: f64,
    in_instrument_id: &str,
    in_quantity: f64,
    out_amount_cents: i64,
    in_amount_cents: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        instrument_id: Some(out_instrument_id.into()),
        quantity: Some(out_quantity),
        to_instrument_id: Some(in_instrument_id.into()),
        to_quantity: Some(in_quantity),
        out_amount_cents: Some(out_amount_cents),
        in_amount_cents: Some(in_amount_cents),
        ..txn_base(TransactionKind::Convert, 0, account_id, date)
    }
}

pub fn dividend_input(
    amount_cents: i64,
    account_id: &str,
    instrument_id: &str,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        instrument_id: Some(instrument_id.into()),
        ..txn_base(TransactionKind::Dividend, amount_cents, account_id, date)
    }
}

pub fn trade_input(
    kind: TransactionKind,
    instrument_id: &str,
    quantity: f64,
    price_cents: i64,
    account_id: &str,
    date: &str,
) -> TransactionInput {
    match kind {
        TransactionKind::Buy => {
            buy_input(instrument_id, quantity, Some(price_cents), account_id, date)
        }
        TransactionKind::Sell => {
            sell_input(instrument_id, quantity, Some(price_cents), account_id, date)
        }
        other => panic!("买卖铺垫仅支持 buy/sell，收到: {other}"),
    }
}

pub fn parse_kind(kind: &str) -> TransactionKind {
    TransactionKind::parse(kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）"))
}

pub fn plain_input(
    kind: TransactionKind,
    amount_cents: i64,
    account_id: &str,
    date: &str,
) -> TransactionInput {
    txn_base(kind, amount_cents, account_id, date)
}

pub fn existing_input(existing: &Transaction) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        kind: existing.kind,
        amount_cents: existing.amount_cents,
        currency_code: existing.currency_code.clone(),
        account_id: existing.account_id.clone(),
        to_account_id: existing.to_account_id.clone(),
        category_id: existing.category_id.clone(),
        merchant_id: existing.merchant_id.clone(),
        policy_id: existing.policy_id.clone(),
        refund_of_transaction_id: existing.refund_of_transaction_id.clone(),
        funding_account_id: None,
        source_order_no: None,
        note: existing.note.clone(),
        date: existing.date.clone(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
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

fn plan_base(
    kind: ScheduledKind,
    amount_cents: i64,
    account_id: &str,
    currency_code: &str,
    start_date: &str,
) -> CreateScheduledInput {
    CreateScheduledInput {
        kind,
        account_id: account_id.into(),
        category_id: None,
        amount_cents,
        currency_code: currency_code.into(),
        recurrence_type: RecurrenceType::Monthly,
        recurrence_interval: 1,
        recurrence_day: None,
        start_date: start_date.into(),
        note: None,
        merchant_id: None,
        policy_id: None,
        total_amount_cents: None,
        total_occurrences: None,
        to_account_id: None,
    }
}

pub fn subscription_plan_input(
    amount_cents: i64,
    account_id: &str,
    currency_code: &str,
    start_date: &str,
) -> CreateScheduledInput {
    plan_base(
        ScheduledKind::Subscription,
        amount_cents,
        account_id,
        currency_code,
        start_date,
    )
}

pub fn installment_plan_input(
    total_amount_cents: i64,
    total_occurrences: i64,
    account_id: &str,
    currency_code: &str,
    start_date: &str,
) -> CreateScheduledInput {
    CreateScheduledInput {
        amount_cents: total_amount_cents / total_occurrences,
        total_amount_cents: Some(total_amount_cents),
        total_occurrences: Some(total_occurrences),
        ..plan_base(
            ScheduledKind::Installment,
            0,
            account_id,
            currency_code,
            start_date,
        )
    }
}

pub fn scheduled_transfer_plan_input(
    amount_cents: i64,
    from_account_id: &str,
    to_account_id: &str,
    currency_code: &str,
    start_date: &str,
) -> CreateScheduledInput {
    CreateScheduledInput {
        to_account_id: Some(to_account_id.into()),
        ..plan_base(
            ScheduledKind::ScheduledTransfer,
            amount_cents,
            from_account_id,
            currency_code,
            start_date,
        )
    }
}
