use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use super::command::{ItemCommand, ItemCommandRow, record_local};
use super::cost;
use super::guard;
use super::guard::apply_purchase_link;
use super::model::{
    Item, ItemDailyCost, ItemDisposeInput, ItemInput, ItemSourceDisplay, ItemStatus,
    ItemWithDailyCost,
};
use ledger_infra::db::query::{query_all, query_one};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;
use ledger_transaction::amount;

fn get_item_by_id(conn: &Connection, id: &str) -> Result<Option<Item>> {
    query_one(
        conn,
        "SELECT id,name,purchase_date,total_cost_cents,currency_code,cost_native_cents,status, \
         disposal_date,residual_value_cents,purchase_transaction_id,note,created_at,updated_at,version,device_id,is_deleted \
         FROM items WHERE id=?1 AND is_deleted=0",
        [id],
    )
}

pub fn list_items(conn: &Connection) -> Result<Vec<ItemWithDailyCost>> {
    let items: Vec<Item> = query_all(
        conn,
        "SELECT id,name,purchase_date,total_cost_cents,currency_code,cost_native_cents,status, \
         disposal_date,residual_value_cents,purchase_transaction_id,note,created_at,updated_at,version,device_id,is_deleted \
         FROM items WHERE is_deleted=0 ORDER BY created_at, id",
        [],
    )?;
    items
        .into_iter()
        .map(|item| {
            let usage = daily_usage(&item)?;
            Ok(ItemWithDailyCost {
                item,
                used_days: usage.days,
                numerator_cents: usage.numerator_cents,
                per_day_cents: usage.per_day_cents,
            })
        })
        .collect()
}

pub fn source_display_by_transaction_ids(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<Vec<ItemSourceDisplay>> {
    if transaction_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; transaction_ids.len()].join(",");
    query_all(
        conn,
        &format!(
            "SELECT id, purchase_transaction_id, name, status FROM items \
             WHERE purchase_transaction_id IN ({placeholders}) AND is_deleted=0",
        ),
        rusqlite::params_from_iter(transaction_ids.iter()),
    )
}

fn usage_to(item: &Item, target_date: chrono::NaiveDate) -> Result<cost::DailyUsageCost> {
    cost::calculate(
        item.total_cost_cents,
        parse_date(&item.purchase_date)?,
        target_date,
        item.residual_value_cents,
    )
}

fn daily_usage(item: &Item) -> Result<cost::DailyUsageCost> {
    match item.status {
        ItemStatus::InUse => usage_to(item, cost::today()),
        ItemStatus::Disposed => {
            let disposal_date = item.disposal_date.as_deref().ok_or_else(|| {
                AppError::codedp(
                    "item.disposal-date-missing",
                    format!("已处置物品缺少处置日期: {}", item.id),
                    &[&item.id],
                )
            })?;
            usage_to(item, parse_date(disposal_date)?)
        }
    }
}

pub fn calculate_item_cost(
    conn: &Connection,
    id: &str,
    reference_date: Option<&str>,
) -> Result<ItemDailyCost> {
    let item = get_item_by_id(conn, id)?.ok_or_else(|| {
        AppError::codedp_not_found("item.not-found", format!("物品不存在: {id}"), &[id])
    })?;
    let usage = match reference_date {
        Some(date) => usage_to(&item, parse_date(date)?),
        None => daily_usage(&item),
    }?;
    Ok(ItemDailyCost {
        used_days: usage.days,
        numerator_cents: usage.numerator_cents,
        per_day_cents: usage.per_day_cents,
    })
}

pub fn create_item(
    conn: &Connection,
    input: ItemInput,
    notify: &mut dyn FnMut(),
) -> Result<String> {
    let id = ensure_transaction(conn, || create_within_transaction(conn, &input))?;
    notify();
    Ok(id)
}

fn create_within_transaction(conn: &Connection, input: &ItemInput) -> Result<String> {
    if input.purchase_transaction_id.is_none() {
        return Err(guard::link_required_error());
    }
    let effective = apply_purchase_link(conn, input)?;
    let link = input.purchase_transaction_id.clone();
    let (name, purchase_date, cost_native_cents) =
        validate_and_convert(conn, &effective, link.as_deref())?;
    let row = ItemCommandRow {
        name,
        purchase_date,
        total_cost_cents: effective.total_cost_cents,
        currency_code: effective.currency_code,
        cost_native_cents,
        purchase_transaction_id: link,
        note: input.note.clone(),
    };
    let id = write_create(conn, &new_uuid(), &row)?;
    record_local(
        conn,
        ItemCommand::Create {
            id: id.clone(),
            row,
        },
    )?;
    Ok(id)
}

fn write_create(conn: &Connection, id: &str, row: &ItemCommandRow) -> Result<String> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO items \
         (id,name,purchase_date,total_cost_cents,currency_code,cost_native_cents,status, \
         disposal_date,residual_value_cents,purchase_transaction_id,note,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,'in_use',NULL,NULL,?7,?8,?9,?10,1,?11,0)",
        rusqlite::params![
            id,
            row.name,
            row.purchase_date,
            row.total_cost_cents,
            row.currency_code,
            row.cost_native_cents,
            row.purchase_transaction_id,
            row.note,
            now,
            now,
            device_id(conn)?,
        ],
    )?;
    Ok(id.to_string())
}

fn validate_and_convert(
    conn: &Connection,
    input: &ItemInput,
    link: Option<&str>,
) -> Result<(String, String, i64)> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::coded("item.name-required", "物品名称不能为空"));
    }
    if input.total_cost_cents <= 0 {
        return Err(AppError::coded(
            "item.cost-positive",
            "物品总成本必须大于 0",
        ));
    }
    let purchase_date = parse_date(&input.purchase_date)?;
    let trade_date = purchase_date.format("%Y-%m-%d").to_string();
    let explicit_rate = linked_fx_rate(conn, link, &input.currency_code)?;
    let conversion = amount::convert_to_native_on_trade_date(
        conn,
        input.total_cost_cents,
        &input.currency_code,
        &trade_date,
        explicit_rate,
    )?;
    Ok((name.to_string(), trade_date, conversion.native_cents))
}

fn linked_fx_rate(
    conn: &Connection,
    link: Option<&str>,
    currency_code: &str,
) -> Result<Option<f64>> {
    if currency_code == amount::default_currency_code(conn)? {
        return Ok(None);
    }
    let Some(tx_id) = link else {
        return Ok(None);
    };
    let row: Option<(String, Option<f64>)> = conn
        .query_row(
            "SELECT currency_code, fx_rate_used FROM transactions \
             WHERE id=?1 AND is_deleted=0",
            rusqlite::params![tx_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(match row {
        Some((tx_currency, rate)) if tx_currency == currency_code => rate,
        _ => None,
    })
}

pub fn update_item(
    conn: &Connection,
    id: &str,
    input: ItemInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    ensure_transaction(conn, || update_within_transaction(conn, id, &input))?;
    notify();
    Ok(())
}

fn update_within_transaction(conn: &Connection, id: &str, input: &ItemInput) -> Result<()> {
    let existing = get_item_by_id(conn, id)?;
    let Some(existing) = existing else {
        return Err(AppError::codedp_not_found(
            "item.not-found",
            format!("物品不存在: {id}"),
            &[id],
        ));
    };

    let effective = match &input.purchase_transaction_id {
        Some(tx_id) if Some(tx_id.as_str()) != existing.purchase_transaction_id.as_deref() => {
            apply_purchase_link(conn, input)?
        }
        _ => input.clone(),
    };
    let link = input
        .purchase_transaction_id
        .clone()
        .or(existing.purchase_transaction_id);

    let (name, purchase_date, cost_native_cents) =
        validate_and_convert(conn, &effective, link.as_deref())?;

    if let Some(disposal_date) = &existing.disposal_date {
        ensure_purchase_not_after_disposal(&purchase_date, disposal_date)?;
    }

    let row = ItemCommandRow {
        name,
        purchase_date,
        total_cost_cents: effective.total_cost_cents,
        currency_code: effective.currency_code,
        cost_native_cents,
        purchase_transaction_id: link,
        note: input.note.clone(),
    };
    write_update_row(conn, id, &row)?;
    record_local(
        conn,
        ItemCommand::Update {
            id: id.to_string(),
            row,
        },
    )
}

fn write_update_row(conn: &Connection, id: &str, row: &ItemCommandRow) -> Result<()> {
    let updated = conn.execute(
        "UPDATE items \
         SET name=?2, purchase_date=?3, total_cost_cents=?4, currency_code=?5, \
         cost_native_cents=?6, purchase_transaction_id=?7, note=?8, updated_at=?9, \
         version=version+1, device_id=?10 \
         WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            row.name,
            row.purchase_date,
            row.total_cost_cents,
            row.currency_code,
            row.cost_native_cents,
            row.purchase_transaction_id,
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

pub fn dispose_item(
    conn: &Connection,
    id: &str,
    input: ItemDisposeInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    ensure_transaction(conn, || {
        let date = write_dispose(conn, id, &input)?;
        record_local(
            conn,
            ItemCommand::Dispose {
                id: id.to_string(),
                disposal_date: date,
                residual_value_cents: input.residual_value_cents,
            },
        )
    })?;
    notify();
    Ok(())
}

fn write_dispose(conn: &Connection, id: &str, input: &ItemDisposeInput) -> Result<String> {
    let Some(existing) = get_item_by_id(conn, id)? else {
        return Err(AppError::coded_not_found(
            "item.not-found",
            format!("物品不存在: {id}"),
        ));
    };

    let disposal_date = parse_date(&input.disposal_date)?;
    let purchase_date = parse_date(&existing.purchase_date)?;
    if disposal_date < purchase_date {
        return Err(AppError::codedp(
            "item.disposal-before-purchase",
            format!("处置日期 {disposal_date} 早于购买日期 {purchase_date}，无法处置"),
            &[&disposal_date.to_string(), &purchase_date.to_string()],
        ));
    }
    if disposal_date > cost::today() {
        return Err(AppError::codedp(
            "item.disposal-in-future",
            format!("处置日期 {disposal_date} 不能晚于今天"),
            &[&disposal_date.to_string()],
        ));
    }
    if input.residual_value_cents.is_some_and(|v| v < 0) {
        return Err(AppError::coded("item.residual-negative", "残值不能为负"));
    }

    let date = disposal_date.format("%Y-%m-%d").to_string();
    let updated = conn.execute(
        "UPDATE items SET status='disposed', disposal_date=?2, residual_value_cents=?3, \
         updated_at=?4, version=version+1, device_id=?5 WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            &date,
            input.residual_value_cents,
            now_iso(),
            device_id(conn)?,
        ],
    )?;
    debug_assert_eq!(
        updated, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
    Ok(date)
}

pub fn delete_item(conn: &Connection, id: &str, notify: &mut dyn FnMut()) -> Result<()> {
    ensure_transaction(conn, || {
        write_delete(conn, id)?;
        record_local(conn, ItemCommand::Delete { id: id.to_string() })
    })?;
    notify();
    Ok(())
}

fn write_delete(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM items WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::coded_not_found(
            "item.not-found",
            format!("物品不存在: {id}"),
        ));
    }
    conn.execute(
        "UPDATE items SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

fn ensure_purchase_not_after_disposal(purchase_date: &str, disposal_date: &str) -> Result<()> {
    let purchase = parse_date(purchase_date)?;
    let disposal = parse_date(disposal_date)?;
    if purchase > disposal {
        return Err(AppError::codedp(
            "item.purchase-after-disposal",
            format!("购买日期 {purchase_date} 晚于处置日期 {disposal_date}，请先调整处置日期"),
            &[purchase_date, disposal_date],
        ));
    }
    Ok(())
}

fn parse_date(s: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        AppError::codedp(
            "item.date-invalid",
            format!("日期格式无效，应为 YYYY-MM-DD: {s}"),
            &[s],
        )
    })
}

pub(crate) fn replay_create(conn: &Connection, id: &str, row: &ItemCommandRow) -> Result<()> {
    let Some(tx_id) = &row.purchase_transaction_id else {
        return Err(guard::link_required_error());
    };
    guard::resolve_purchase_link(conn, tx_id)?;
    write_create(conn, id, row)?;
    Ok(())
}

pub(crate) fn replay_update(conn: &Connection, id: &str, row: &ItemCommandRow) -> Result<()> {
    let existing = get_item_by_id(conn, id)?.ok_or_else(|| {
        AppError::codedp_not_found("item.not-found", format!("物品不存在: {id}"), &[id])
    })?;
    if let Some(tx_id) = &row.purchase_transaction_id
        && Some(tx_id.as_str()) != existing.purchase_transaction_id.as_deref()
    {
        guard::resolve_purchase_link(conn, tx_id)?;
    }
    if let Some(disposal_date) = &existing.disposal_date {
        ensure_purchase_not_after_disposal(&row.purchase_date, disposal_date)?;
    }
    write_update_row(conn, id, row)
}

pub(crate) fn replay_dispose(conn: &Connection, id: &str, input: &ItemDisposeInput) -> Result<()> {
    write_dispose(conn, id, input)?;
    Ok(())
}

pub(crate) fn replay_delete(conn: &Connection, id: &str) -> Result<()> {
    write_delete(conn, id)
}

fn item_source_resolver(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, ledger_transaction::TransactionSource>> {
    Ok(source_display_by_transaction_ids(conn, transaction_ids)?
        .into_iter()
        .filter_map(|row| {
            let txn_id = row.purchase_transaction_id.clone()?;
            Some((
                txn_id,
                ledger_transaction::TransactionSource {
                    kind: ledger_transaction::TransactionSourceKind::Item,
                    entity_id: row.id.clone(),
                    display_name: row.name.clone(),
                    status: row
                        .is_disposed
                        .then_some(ledger_transaction::TransactionSourceStatus::Disposed),
                },
            ))
        })
        .collect())
}

pub fn install_source_hook() {
    ledger_transaction::seams::source::register_item_source_resolver(item_source_resolver);
}
