#![allow(clippy::unreachable)]

use tauri::{AppHandle, Runtime, State};

use crate::shell_support::read_entry::{read_entry, read_entry_on_write};
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_accounts as account_domain;
use ledger_accounts::{
    Account, AccountBalance, AccountBalanceAdjustInput, AccountInput, AccountUpdateInput,
    BalanceCacheAudit,
};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::{WriteEvidence, WriteOp};

#[tauri::command]
pub async fn list_accounts(db: State<'_, DbState>) -> Result<Vec<Account>> {
    let conn = db.read_handle();
    read_entry("list_accounts", conn, move |conn| {
        account_domain::list_accounts(conn)
    })
    .await
}

#[tauri::command]
pub async fn create_account<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
    input: AccountInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "create_account",
        conn,
        Some(&app),
        WriteOp::CreateAccount,
        move |conn| account_domain::create_account(conn, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn delete_account<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
    id: String,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "delete_account",
        conn,
        Some(&app),
        WriteOp::DeleteAccount,
        move |conn| {
            ledger_savings_goal::ensure_account_not_goal_bound(conn, &id)?;
            account_domain::delete_account(conn, &id).map(Outcome::Silent)
        },
    )
    .await
}

#[tauri::command]
pub async fn update_account(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: AccountUpdateInput,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "update_account",
        conn,
        Some(&app),
        WriteOp::UpdateAccount,
        move |conn| {
            if input.is_archived == Some(true) {
                ledger_savings_goal::ensure_account_can_archive(conn, &id)?;
            }
            account_domain::update_account(conn, &id, input).map(Outcome::Silent)
        },
    )
    .await
}

#[tauri::command]
pub async fn archive_account<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
    id: String,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "archive_account",
        conn,
        Some(&app),
        WriteOp::ArchiveAccount,
        move |conn| {
            ledger_savings_goal::ensure_account_can_archive(conn, &id)?;
            account_domain::set_account_archived(conn, &id, true).map(Outcome::Silent)
        },
    )
    .await
}

#[tauri::command]
pub async fn unarchive_account<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
    id: String,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "unarchive_account",
        conn,
        Some(&app),
        WriteOp::UnarchiveAccount,
        move |conn| account_domain::set_account_archived(conn, &id, false).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn adjust_account_balance(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: AccountBalanceAdjustInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "adjust_account_balance",
        conn,
        Some(&app),
        WriteOp::AdjustAccountBalance,
        move |conn| {
            account_domain::adjust_account_balance(conn, &id, &input).map(|(tx_id, created)| {
                Outcome::Evidenced(tx_id, WriteEvidence::BlackHoleCreated(created))
            })
        },
    )
    .await
}

#[tauri::command]
pub async fn list_account_balances(db: State<'_, DbState>) -> Result<Vec<AccountBalance>> {
    let conn = db.read_handle();
    read_entry("list_account_balances", conn, move |conn| {
        account_domain::list_account_balances_with_visibility(conn, false)
    })
    .await
}

#[tauri::command]
pub async fn audit_balance_cache(db: State<'_, DbState>) -> Result<BalanceCacheAudit> {
    let conn = db.write_handle();
    read_entry_on_write("audit_balance_cache", conn, move |conn| {
        account_domain::audit_balance_cache(conn)
    })
    .await
}
