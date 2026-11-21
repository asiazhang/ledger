use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::{ensure_transaction, hold_transaction};
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;
use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::create_transaction_internal;
use rusqlite::{Connection, OptionalExtension};

use super::command::{AccountCommand, AccountCommandRow, record_local};
use super::model::{
    Account, AccountBalance, AccountBalanceAdjustInput, AccountInput, AccountType,
    AccountUpdateInput, BalanceCacheAudit, BalanceCacheDrift, CreditTerms,
};
use crate::balance::refresh_account_balances;

pub fn list_accounts(conn: &Connection) -> Result<Vec<Account>> {
    crate::balance::list_accounts_with_visibility(conn, false)
}

const FUNDING_CASH_LIKE_TYPES: [AccountType; 5] = [
    AccountType::Cash,
    AccountType::Bank,
    AccountType::Credit,
    AccountType::Ewallet,
    AccountType::Other,
];

fn funding_account_view(
    conn: &Connection,
    id: &str,
) -> Result<Option<ledger_transaction::seams::funding::FundingAccountView>> {
    let (account_type, currency_code): (AccountType, String) = match conn.query_row(
        "SELECT type, currency_code FROM accounts WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ) {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    Ok(Some(
        ledger_transaction::seams::funding::FundingAccountView {
            class: if FUNDING_CASH_LIKE_TYPES.contains(&account_type) {
                ledger_transaction::seams::funding::FundingAccountClass::CashLike
            } else {
                ledger_transaction::seams::funding::FundingAccountClass::Ineligible
            },
            type_display: account_type.to_string(),
            currency_code,
        },
    ))
}

pub fn install_funding_account_hook() {
    ledger_transaction::seams::funding::register_funding_account_lookup_hook(funding_account_view);
}

pub fn list_accounts_for_api(conn: &Connection) -> Result<Vec<Account>> {
    crate::balance::list_accounts_with_visibility(conn, true)
}

pub fn create_account(conn: &Connection, input: AccountInput) -> Result<String> {
    ensure_transaction(conn, || create_within_transaction(conn, &input))
}

fn create_within_transaction(conn: &Connection, input: &AccountInput) -> Result<String> {
    let row = AccountCommandRow {
        name: input.name.clone(),
        kind: input.kind,
        currency_code: input.currency_code.clone(),
        initial_balance_cents: input.initial_balance_cents.unwrap_or(0),
        is_hidden: false,
        credit_limit_cents: input.credit_limit_cents,
        statement_day: input.statement_day,
        due_day: input.due_day,
    };
    let id = write_create(conn, &new_uuid(), &row)?;
    record_local(
        conn,
        AccountCommand::Create {
            id: id.clone(),
            row,
        },
    )?;
    Ok(id)
}

fn write_create(conn: &Connection, id: &str, row: &AccountCommandRow) -> Result<String> {
    CreditTerms::new(row.credit_limit_cents, row.statement_day, row.due_day)
        .validate_for(row.kind)?;
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden,is_archived,credit_limit_cents,statement_day,due_day) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8,0,?9,0,?10,?11,?12)",
        rusqlite::params![
            id,
            row.name,
            row.kind,
            row.currency_code,
            row.initial_balance_cents,
            now,
            now,
            device_id(conn)?,
            row.is_hidden,
            row.credit_limit_cents,
            row.statement_day,
            row.due_day,
        ],
    )?;
    refresh_account_balances(conn, &[id])?;
    Ok(id.to_string())
}

pub fn create_account_idempotent(conn: &Connection, input: AccountInput) -> Result<String> {
    if let Some(id) = find_account_by_natural_key(conn, &input)? {
        return Ok(id);
    }
    create_account(conn, input)
}

fn find_account_by_natural_key(conn: &Connection, input: &AccountInput) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM accounts \
         WHERE name=?1 AND type=?2 AND currency_code=?3 AND is_deleted=0 LIMIT 1",
    )?;
    let mut rows = stmt.query(rusqlite::params![
        input.name,
        input.kind,
        input.currency_code
    ])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

pub fn delete_account(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        write_delete(conn, id)?;
        record_local(conn, AccountCommand::Delete { id: id.to_string() })
    })
}

pub fn set_account_archived(conn: &Connection, id: &str, archived: bool) -> Result<()> {
    ensure_transaction(conn, || {
        if !apply_archive(conn, id, archived)? {
            return Ok(());
        }
        record_local(
            conn,
            AccountCommand::SetArchived {
                id: id.to_string(),
                archived,
            },
        )
    })
}

fn apply_archive(conn: &Connection, id: &str, archived: bool) -> Result<bool> {
    if current_archived(conn, id)? == archived {
        return Ok(false);
    }
    write_set_archived(conn, id, archived)?;
    Ok(true)
}

fn current_archived(conn: &Connection, id: &str) -> Result<bool> {
    conn.query_row(
        "SELECT is_archived FROM accounts WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
        |r| r.get::<_, i64>(0),
    )
    .map(|v| v != 0)
    .optional()?
    .ok_or_else(|| {
        AppError::codedp_not_found("account.not-found", format!("账户不存在: {id}"), &[id])
    })
}

fn write_set_archived(conn: &Connection, id: &str, archived: bool) -> Result<()> {
    conn.execute(
        "UPDATE accounts SET is_archived=?2, updated_at=?3, version=version+1, device_id=?4 WHERE id=?1",
        rusqlite::params![id, archived, now_iso(), device_id(conn)?],
    )?;
    refresh_account_balances(conn, &[id])?;
    Ok(())
}

fn write_delete(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM accounts WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::coded_not_found(
            "account.not-found",
            format!("账户不存在: {id}"),
        ));
    }
    conn.execute(
        "UPDATE accounts SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    refresh_account_balances(conn, &[id])?;
    Ok(())
}

pub fn get_account(conn: &Connection, id: &str) -> Result<Account> {
    query_all(
        conn,
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden,is_archived,credit_limit_cents,statement_day,due_day \
         FROM accounts WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| AppError::codedp_not_found("account.not-found", format!("账户不存在: {id}"), &[id]))
}

pub fn update_account(conn: &Connection, id: &str, input: AccountUpdateInput) -> Result<()> {
    let archived = input.is_archived;
    let has_edits = input.name.is_some()
        || input.currency_code.is_some()
        || input.credit_limit_cents.is_some()
        || input.statement_day.is_some()
        || input.due_day.is_some();
    ensure_transaction(conn, || {
        if has_edits {
            let (name, currency_code, credit_terms) = write_update(conn, id, &input)?;
            record_local(
                conn,
                AccountCommand::Update {
                    id: id.to_string(),
                    name,
                    currency_code,
                    credit_limit_cents: credit_terms.credit_limit_cents,
                    statement_day: credit_terms.statement_day,
                    due_day: credit_terms.due_day,
                },
            )?;
        }
        if let Some(archived) = archived {
            set_account_archived(conn, id, archived)?;
        }
        Ok(())
    })
}

fn write_update(
    conn: &Connection,
    id: &str,
    input: &AccountUpdateInput,
) -> Result<(String, String, CreditTerms)> {
    let existing = get_account(conn, id)?;
    let carried = input.credit_limit_cents.is_some()
        || input.statement_day.is_some()
        || input.due_day.is_some();
    let credit_terms = CreditTerms::new(
        input
            .credit_limit_cents
            .unwrap_or(existing.credit_limit_cents),
        input.statement_day.unwrap_or(existing.statement_day),
        input.due_day.unwrap_or(existing.due_day),
    );
    if carried {
        credit_terms.validate_for(existing.kind)?;
    }
    let name = match input.name {
        Some(ref n) => {
            let trimmed = n.trim();
            if trimmed.is_empty() {
                return Err(AppError::coded("account.name-required", "账户名称不能为空"));
            }
            trimmed.to_string()
        }
        None => existing.name.clone(),
    };
    let currency_code = match input.currency_code {
        Some(ref code) if code != &existing.currency_code => {
            let referenced: bool = conn.query_row(
                "SELECT CASE WHEN \
                     EXISTS(SELECT 1 FROM transactions WHERE account_id=?1 AND is_deleted=0) \
                     OR EXISTS(SELECT 1 FROM transactions WHERE to_account_id=?1 AND is_deleted=0) \
                     THEN 1 ELSE 0 END",
                rusqlite::params![id],
                |r| r.get(0),
            )?;
            if referenced {
                return Err(AppError::coded(
                    "account.currency-locked",
                    "账户已有交易，不能修改币种（会使历史交易折算口径错乱）",
                ));
            }
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM currencies WHERE code=?1",
                    rusqlite::params![code],
                    |_| Ok(true),
                )
                .optional()?
                .is_some();
            if !exists {
                return Err(AppError::codedp(
                    "account.currency-unknown",
                    format!("未知币种: {code}"),
                    &[code.as_str()],
                ));
            }
            code.clone()
        }
        _ => existing.currency_code.clone(),
    };
    conn.execute(
        "UPDATE accounts SET name=?2, currency_code=?3, credit_limit_cents=?4, statement_day=?5, due_day=?6, updated_at=?7, version=version+1, device_id=?8 WHERE id=?1",
        rusqlite::params![
            id,
            &name,
            &currency_code,
            credit_terms.credit_limit_cents,
            credit_terms.statement_day,
            credit_terms.due_day,
            now_iso(),
            device_id(conn)?
        ],
    )?;
    refresh_account_balances(conn, &[id])?;
    Ok((name, currency_code, credit_terms))
}

pub(crate) fn replay_create(conn: &Connection, id: &str, row: &AccountCommandRow) -> Result<()> {
    write_create(conn, id, row)?;
    Ok(())
}

pub(crate) fn replay_update(
    conn: &Connection,
    id: &str,
    name: &str,
    currency_code: &str,
    credit_limit_cents: Option<i64>,
    statement_day: Option<i64>,
    due_day: Option<i64>,
) -> Result<()> {
    write_update(
        conn,
        id,
        &AccountUpdateInput {
            name: Some(name.to_string()),
            currency_code: Some(currency_code.to_string()),
            credit_limit_cents: Some(credit_limit_cents),
            statement_day: Some(statement_day),
            due_day: Some(due_day),
            is_archived: None,
        },
    )?;
    Ok(())
}

pub(crate) fn replay_delete(conn: &Connection, id: &str) -> Result<()> {
    write_delete(conn, id)
}

pub(crate) fn replay_set_archived(conn: &Connection, id: &str, archived: bool) -> Result<()> {
    apply_archive(conn, id, archived).map(|_wrote| ())
}

fn find_black_hole_account(conn: &Connection, currency_code: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM accounts WHERE is_deleted=0 AND is_hidden=1 AND currency_code=?1 \
         ORDER BY created_at LIMIT 1",
    )?;
    let mut rows = stmt.query(rusqlite::params![currency_code])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

pub fn ensure_black_hole_account(conn: &Connection, currency_code: &str) -> Result<(String, bool)> {
    if let Some(id) = find_black_hole_account(conn, currency_code)? {
        return Ok((id, false));
    }
    let row = AccountCommandRow {
        name: format!("无({currency_code})"),
        kind: AccountType::Other,
        currency_code: currency_code.to_string(),
        initial_balance_cents: 0,
        is_hidden: true,
        credit_limit_cents: None,
        statement_day: None,
        due_day: None,
    };
    let id = write_create(conn, &new_uuid(), &row)?;
    record_local(
        conn,
        AccountCommand::Create {
            id: id.clone(),
            row,
        },
    )?;
    Ok((id, true))
}

pub fn adjust_account_balance(
    conn: &Connection,
    id: &str,
    input: &AccountBalanceAdjustInput,
) -> Result<(String, bool)> {
    let account = get_account(conn, id)?;
    if account.is_hidden {
        return Err(AppError::coded(
            "account.black-hole-adjust-unsupported",
            "黑洞账户不支持余额调整",
        ));
    }
    let current = crate::balance::cached_balance(conn, id)?;
    let delta = input
        .target_balance_cents
        .checked_sub(current)
        .ok_or_else(|| AppError::coded("account.balance-overflow", "目标余额溢出"))?;
    if delta == 0 {
        return Err(AppError::coded(
            "account.balance-no-change",
            "余额已等于目标值，无需调整",
        ));
    }
    hold_transaction(conn, || {
        let (black_hole_id, created) = ensure_black_hole_account(conn, &account.currency_code)?;
        let (account_id, to_account_id) = if delta > 0 {
            (black_hole_id.clone(), id.to_string())
        } else {
            (id.to_string(), black_hole_id.clone())
        };
        let tx_id = create_transaction_internal(
            conn,
            TransactionInput {
                kind: TransactionKind::Transfer,
                amount_cents: delta.abs(),
                currency_code: account.currency_code.clone(),
                account_id: Some(account_id),
                to_account_id: Some(to_account_id),
                category_id: None,
                merchant_id: None,
                merchant_name: None,
                policy_id: None,
                refund_of_transaction_id: None,
                source_order_no: None,
                funding_account_id: None,
                funding: Vec::new(),
                purchases: Vec::new(),
                note: Some(input.note.clone().unwrap_or_else(|| "余额调整".to_string())),
                date: input.date.clone(),
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
            },
        )?
        .id;
        Ok((tx_id, created))
    })
}

pub fn audit_balance_cache(conn: &Connection) -> Result<BalanceCacheAudit> {
    let accounts = crate::balance::list_accounts_with_visibility(conn, true)?;
    let mut drifts = Vec::new();
    for account in &accounts {
        let actual = crate::balance::compute_balance(conn, &account.id)?;
        let cached = crate::balance::cached_balance_optional(conn, &account.id)?;
        if cached != Some(actual) {
            drifts.push(BalanceCacheDrift {
                account_id: account.id.clone(),
                account_name: account.name.clone(),
                cached_cents: cached,
                actual_cents: actual,
            });
        }
    }
    let repaired = !drifts.is_empty();
    if repaired {
        crate::balance::refresh_all_account_balances(conn)?;
    }
    Ok(BalanceCacheAudit {
        accounts_checked: accounts.len(),
        drifts,
        repaired,
    })
}

pub fn list_account_balances_with_visibility(
    conn: &Connection,
    include_hidden: bool,
) -> Result<Vec<AccountBalance>> {
    crate::balance::list_account_balances_with_visibility(conn, include_hidden)
}

pub fn list_account_balances_for_api(conn: &Connection) -> Result<Vec<AccountBalance>> {
    crate::balance::list_account_balances_with_visibility(conn, true)
}
