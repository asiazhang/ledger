use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use super::command::{PolicyCommand, PolicyCommandRow, record_policy_local};
use super::model::{Policy, PolicyInput, PolicySourceDisplay};
use ledger_infra::db::query::{query_all, query_one};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;

use super::validation::validate_input;

const POLICY_COLUMNS: &str = "id,insurer_id,policy_number,product_name,start_date,end_date,\
     coverage_amount_cents,coverage_currency_code,note,created_at,updated_at,version,device_id,is_deleted";

fn get_policy_by_id(conn: &Connection, id: &str) -> Result<Option<Policy>> {
    query_one(
        conn,
        &format!("SELECT {POLICY_COLUMNS} FROM policies WHERE id=?1 AND is_deleted=0"),
        [id],
    )
}

pub fn list_policies(conn: &Connection) -> Result<Vec<Policy>> {
    query_all(
        conn,
        &format!(
            "SELECT {POLICY_COLUMNS} FROM policies WHERE is_deleted=0 ORDER BY created_at, id"
        ),
        [],
    )
}

pub fn source_display_by_ids(
    conn: &Connection,
    ids: &[String],
) -> Result<Vec<PolicySourceDisplay>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    query_all(
        conn,
        &format!("SELECT id, product_name, is_deleted FROM policies WHERE id IN ({placeholders})"),
        rusqlite::params_from_iter(ids.iter()),
    )
}

pub fn create_policy(
    conn: &Connection,
    input: PolicyInput,
    notify: &mut dyn FnMut(),
) -> Result<String> {
    let id = ensure_transaction(conn, || {
        let normalized = validate_input(conn, &input, false)?;
        let row = PolicyCommandRow::from(&normalized);
        let id = write_create(conn, &new_uuid(), &row)?;
        record_policy_local(
            conn,
            PolicyCommand::Create {
                id: id.clone(),
                row,
            },
        )?;
        Ok(id)
    })?;
    notify();
    Ok(id)
}

fn write_create(conn: &Connection, id: &str, row: &PolicyCommandRow) -> Result<String> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO policies \
         (id,insurer_id,policy_number,product_name,start_date,end_date,\
         coverage_amount_cents,coverage_currency_code,note,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10,1,?11,0)",
        rusqlite::params![
            id,
            row.insurer_id,
            row.policy_number,
            row.product_name,
            row.start_date,
            row.end_date,
            row.coverage_amount_cents,
            row.coverage_currency_code,
            row.note,
            now,
            device_id(conn)?,
        ],
    )?;
    Ok(id.to_string())
}

pub fn update_policy(
    conn: &Connection,
    id: &str,
    input: PolicyInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    ensure_transaction(conn, || {
        let existing = get_policy_by_id(conn, id)?.ok_or_else(|| {
            AppError::codedp_not_found("policy.not-found", format!("保单不存在: {id}"), &[id])
        })?;

        let insurer_unchanged = existing.insurer_id == input.insurer_id;
        let normalized = validate_input(conn, &input, insurer_unchanged)?;
        let row = PolicyCommandRow::from(&normalized);
        write_update(conn, id, &row)?;
        record_policy_local(
            conn,
            PolicyCommand::Update {
                id: id.to_string(),
                row,
            },
        )
    })?;
    notify();
    Ok(())
}

fn write_update(conn: &Connection, id: &str, row: &PolicyCommandRow) -> Result<()> {
    let updated = conn.execute(
        "UPDATE policies SET insurer_id=?2, policy_number=?3, product_name=?4, start_date=?5, \
         end_date=?6, coverage_amount_cents=?7, coverage_currency_code=?8, note=?9, \
         updated_at=?10, version=version+1, device_id=?11 WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            row.insurer_id,
            row.policy_number,
            row.product_name,
            row.start_date,
            row.end_date,
            row.coverage_amount_cents,
            row.coverage_currency_code,
            row.note,
            now_iso(),
            device_id(conn)?,
        ],
    )?;
    debug_assert_eq!(
        updated, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
    Ok(())
}

pub fn delete_policy(conn: &Connection, id: &str, notify: &mut dyn FnMut()) -> Result<()> {
    ensure_transaction(conn, || {
        write_delete(conn, id)?;
        record_policy_local(conn, PolicyCommand::Delete { id: id.to_string() })
    })?;
    notify();
    Ok(())
}

fn write_delete(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM policies WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::coded_not_found(
            "policy.not-found",
            format!("保单不存在: {id}"),
        ));
    }
    conn.execute(
        "UPDATE policies SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

pub(crate) fn replay_create(conn: &Connection, id: &str, row: &PolicyCommandRow) -> Result<()> {
    validate_input(conn, &PolicyInput::from(row), false)?;
    write_create(conn, id, row)?;
    Ok(())
}

pub(crate) fn replay_update(conn: &Connection, id: &str, row: &PolicyCommandRow) -> Result<()> {
    let existing = get_policy_by_id(conn, id)?.ok_or_else(|| {
        AppError::codedp_not_found("policy.not-found", format!("保单不存在: {id}"), &[id])
    })?;
    let insurer_unchanged = existing.insurer_id == row.insurer_id;
    validate_input(conn, &PolicyInput::from(row), insurer_unchanged)?;
    write_update(conn, id, row)
}

pub(crate) fn replay_delete(conn: &Connection, id: &str) -> Result<()> {
    write_delete(conn, id)
}

fn policy_source_resolver(
    conn: &Connection,
    policy_ids: &[String],
) -> Result<HashMap<String, ledger_transaction::TransactionSource>> {
    Ok(source_display_by_ids(conn, policy_ids)?
        .into_iter()
        .map(|row| {
            (
                row.id.clone(),
                ledger_transaction::TransactionSource {
                    kind: ledger_transaction::TransactionSourceKind::Policy,
                    entity_id: row.id.clone(),
                    display_name: row.product_name.clone(),
                    status: row
                        .is_deleted
                        .then_some(ledger_transaction::TransactionSourceStatus::Deleted),
                },
            )
        })
        .collect())
}

pub fn install_source_hook() {
    ledger_transaction::seams::source::register_policy_source_resolver(policy_source_resolver);
}
