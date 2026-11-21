use std::collections::HashMap;

use rusqlite::Connection;

use ledger_infra::error::Result;

use crate::amount::TransactionKind;
use crate::model::{ConvertFields, Transaction};
use crate::seams::investment::{resolve_convert_fields, resolve_instrument_sources};
use crate::seams::source::{resolve_item_sources, resolve_plan_sources, resolve_policy_sources};

pub(super) fn attach_sources(conn: &Connection, items: &mut [Transaction]) -> Result<()> {
    let mut policy_ids: Vec<String> = Vec::new();
    for txn in items.iter() {
        if let Some(pid) = txn.policy_id.as_ref()
            && !policy_ids.contains(pid)
        {
            policy_ids.push(pid.clone());
        }
    }
    if !policy_ids.is_empty() {
        let by_id = resolve_policy_sources(conn, &policy_ids)?;
        for txn in items.iter_mut() {
            let Some(pid) = txn.policy_id.as_deref() else {
                continue;
            };
            let Some(source) = by_id.get(pid) else {
                continue;
            };
            txn.source = Some(source.clone());
        }
    }

    let plan_txn_ids: Vec<String> = items
        .iter()
        .filter(|t| t.source.is_none())
        .map(|t| t.id.clone())
        .collect();
    if !plan_txn_ids.is_empty() {
        let by_txn = resolve_plan_sources(conn, &plan_txn_ids)?;
        for txn in items.iter_mut() {
            if txn.source.is_some() {
                continue;
            }
            if let Some(source) = by_txn.get(txn.id.as_str()) {
                txn.source = Some(source.clone());
            }
        }
    }

    let item_txn_ids: Vec<String> = items
        .iter()
        .filter(|t| t.source.is_none())
        .map(|t| t.id.clone())
        .collect();
    if !item_txn_ids.is_empty() {
        let by_txn = resolve_item_sources(conn, &item_txn_ids)?;
        for txn in items.iter_mut() {
            if txn.source.is_some() {
                continue;
            }
            if let Some(source) = by_txn.get(txn.id.as_str()) {
                txn.source = Some(source.clone());
            }
        }
    }

    let instrument_txn_ids: Vec<String> = items
        .iter()
        .filter(|t| t.source.is_none())
        .map(|t| t.id.clone())
        .collect();
    if !instrument_txn_ids.is_empty() {
        let by_txn = resolve_instrument_sources(conn, &instrument_txn_ids)?;
        for txn in items.iter_mut() {
            if txn.source.is_some() {
                continue;
            }
            if let Some(source) = by_txn.get(txn.id.as_str()) {
                txn.source = Some(source.clone());
            }
        }
    }
    Ok(())
}

pub(super) fn attach_convert_fields(conn: &Connection, items: &mut [Transaction]) -> Result<()> {
    let convert_ids: Vec<String> = items
        .iter()
        .filter(|t| t.kind == TransactionKind::Convert)
        .map(|t| t.id.clone())
        .collect();
    let by_txn: HashMap<String, ConvertFields> = resolve_convert_fields(conn, &convert_ids)?;
    for txn in items.iter_mut() {
        if let Some(fields) = by_txn.get(txn.id.as_str()) {
            txn.convert = Some(fields.clone());
        }
    }
    Ok(())
}
