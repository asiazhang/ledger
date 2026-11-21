use ledger_accounts::{AccountInput, AccountType, create_account};
use ledger_currencies::ExchangeRateInput;
use ledger_infra::error::AppError;
use ledger_investment::create_exchange_rate;
use ledger_scheduled::{CreateScheduledInput, ScheduledStatus, create_plan, update_plan_status};
use ledger_transaction::{
    TransactionInput, TransactionWrite, create_transaction, delete_transaction, update_transaction,
};

use crate::step_inputs::{
    installment_plan_input, refund_input, scheduled_transfer_plan_input, subscription_plan_input,
    transfer_input,
};
use crate::world::LedgerWorld;

pub fn create_account_verb(
    world: &mut LedgerWorld,
    name: &str,
    kind: &str,
    currency: &str,
    initial_balance_cents: Option<i64>,
) -> String {
    try_create_account_verb(world, name, kind, currency, initial_balance_cents)
        .expect("创建账户失败")
}

pub fn try_create_account_verb(
    world: &mut LedgerWorld,
    name: &str,
    kind: &str,
    currency: &str,
    initial_balance_cents: Option<i64>,
) -> Result<String, AppError> {
    let input = AccountInput {
        name: name.into(),
        kind: kind
            .parse::<AccountType>()
            .map_err(|e| AppError::Invalid(format!("非法账户类型 {kind}: {e}")))?,
        currency_code: currency.into(),
        initial_balance_cents,
        credit_limit_cents: None,
        statement_day: None,
        due_day: None,
    };
    let id = create_account(&world_conn!(world), input)?;
    world.account_name_to_id.insert(name.into(), id.clone());
    Ok(id)
}

pub fn create_transaction_verb(world: &mut LedgerWorld, input: TransactionInput) -> String {
    try_create_transaction_verb(world, input).expect("创建交易失败")
}

pub fn try_create_transaction_verb(
    world: &mut LedgerWorld,
    input: TransactionInput,
) -> Result<String, AppError> {
    let write: TransactionWrite = world_write!(world, |conn| create_transaction(conn, input))?;
    world.txn.last_transaction_id = Some(write.id.clone());
    Ok(write.id)
}

pub fn create_transfer(
    world: &mut LedgerWorld,
    amount_cents: i64,
    from: &str,
    to: &str,
    date: &str,
) -> String {
    let from_id = world.account_id(from);
    let to_id = world.account_id(to);
    create_transaction_verb(world, transfer_input(amount_cents, &from_id, &to_id, date))
}

pub fn create_refund(
    world: &mut LedgerWorld,
    amount_cents: i64,
    original_id: &str,
    date: &str,
) -> String {
    let account_id = transaction_account_id(world, original_id);
    create_transaction_verb(
        world,
        refund_input(amount_cents, &account_id, original_id, date),
    )
}

pub fn refund_last_transaction(world: &mut LedgerWorld, amount_cents: i64, date: &str) -> String {
    let original_id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("没有上一笔交易可关联退款");
    create_refund(world, amount_cents, &original_id, date)
}

fn transaction_account_id(world: &LedgerWorld, transaction_id: &str) -> String {
    let conn = world_conn!(world);
    conn.query_row(
        "SELECT account_id FROM transactions WHERE id=?1",
        [transaction_id],
        |r| r.get(0),
    )
    .expect("查询原交易账户失败")
}

pub fn update_transaction_verb(world: &mut LedgerWorld, id: &str, input: TransactionInput) {
    try_update_transaction_verb(world, id, input).expect("修改交易失败");
}

pub fn try_update_transaction_verb(
    world: &mut LedgerWorld,
    id: &str,
    input: TransactionInput,
) -> Result<(), AppError> {
    world_write!(world, |conn| update_transaction(conn, id, input)).map(|_| ())
}

pub fn delete_transaction_verb(world: &mut LedgerWorld, id: &str) {
    try_delete_transaction_verb(world, id).expect("删除交易失败");
}

pub fn try_delete_transaction_verb(world: &mut LedgerWorld, id: &str) -> Result<(), AppError> {
    world_write!(world, |conn| delete_transaction(conn, id))
}

pub fn create_plan_verb(world: &mut LedgerWorld, input: CreateScheduledInput) -> String {
    try_create_plan_verb(world, input).expect("创建计划失败")
}

pub fn try_create_plan_verb(
    world: &mut LedgerWorld,
    input: CreateScheduledInput,
) -> Result<String, AppError> {
    let id = world_write!(world, |conn| create_plan(conn, input))?;
    world.plan.last_plan_id = Some(id.clone());
    Ok(id)
}

pub fn create_subscription_plan(
    world: &mut LedgerWorld,
    amount_cents: i64,
    currency: &str,
    account: &str,
    start: &str,
) -> String {
    let account_id = world.account_id(account);
    create_plan_verb(
        world,
        subscription_plan_input(amount_cents, &account_id, currency, start),
    )
}

pub fn create_installment_plan(
    world: &mut LedgerWorld,
    total_amount_cents: i64,
    total_occurrences: i64,
    account: &str,
    start: &str,
) -> String {
    let account_id = world.account_id(account);
    let currency = account_currency_code(world, &account_id);
    create_plan_verb(
        world,
        installment_plan_input(
            total_amount_cents,
            total_occurrences,
            &account_id,
            &currency,
            start,
        ),
    )
}

pub fn create_scheduled_transfer_plan(
    world: &mut LedgerWorld,
    amount_cents: i64,
    from: &str,
    to: &str,
    total_occurrences: Option<i64>,
    start: &str,
) -> String {
    let from_id = world.account_id(from);
    let to_id = world.account_id(to);
    let currency = account_currency_code(world, &from_id);
    create_plan_verb(
        world,
        CreateScheduledInput {
            total_occurrences,
            ..scheduled_transfer_plan_input(amount_cents, &from_id, &to_id, &currency, start)
        },
    )
}

pub(crate) fn account_currency_code(world: &LedgerWorld, account_id: &str) -> String {
    let conn = world_conn!(world);
    conn.query_row(
        "SELECT currency_code FROM accounts WHERE id=?1",
        [account_id],
        |r| r.get(0),
    )
    .expect("查询账户币种失败")
}

pub fn update_plan_status_verb(world: &mut LedgerWorld, id: &str, status: ScheduledStatus) {
    world_write!(world, |conn| update_plan_status(conn, id, status)).expect("计划状态变更失败");
}

pub fn seed_fx_history_series_verb(
    world: &mut LedgerWorld,
    base: &str,
    quote: &str,
    rate: f64,
    from_date: &str,
) {
    use chrono::Datelike;
    let mut d = chrono::NaiveDate::parse_from_str(from_date, "%Y-%m-%d")
        .expect("汇率历史夹具起始日期须为 YYYY-MM-DD");
    let today = chrono::Local::now().date_naive() + chrono::Duration::days(8);
    while d <= today {
        let date = d.format("%Y-%m-%d").to_string();
        tauri_app_lib::test_support::seed_fx_rate_history(
            &world_conn!(world),
            &format!("fxh-{base}-{quote}-{}", d.format("%Y%m%d")),
            base,
            quote,
            &date,
            rate,
        );
        d = d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64)
            + chrono::Duration::weeks(1);
    }
}

pub fn create_exchange_rate_verb(
    world: &mut LedgerWorld,
    base: &str,
    quote: &str,
    rate: f64,
    priced_at: &str,
) {
    let input = ExchangeRateInput {
        base_code: base.into(),
        quote_code: quote.into(),
        rate,
        priced_at: priced_at.into(),
        source: Some("manual".into()),
    };
    create_exchange_rate(&world_conn!(world), input).expect("存在汇率夹具：写入失败");
}
