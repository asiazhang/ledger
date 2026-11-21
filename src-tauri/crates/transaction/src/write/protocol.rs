use rusqlite::Connection;
use rusqlite::OptionalExtension;

use crate::command::{
    ConvertCommandFields, InvestmentCommandFields, SplitCommandFields, TransactionCommand,
};
use crate::model::{NormalizedTransaction, TransactionInput};
use crate::seams::investment::{
    InvestmentPlan, PlanCommandParts, prepare_investment, release_for_delete_investment,
    replay_investment, revert_investment,
};
use crate::seams::merchant as merchant_hooks;
use ledger_infra::db::now_iso;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::{AppError, Result};
use ledger_infra::signals::WriteEvidence;
use ledger_sync_protocol::device::device_id;

use crate::amount::{FxEditBaseline, NativeConversion, TransactionKind};
use crate::write::op::record_local;
use crate::write::writer;

pub use create as create_transaction;
pub use create as create_transaction_internal;
pub use delete as delete_transaction;
pub use delete as delete_transaction_internal;
pub use update as update_transaction;
pub use update as update_transaction_internal;

const CONVERT_KIND_CHANGE_FORBIDDEN_CODE: &str = "trade.convert-kind-change-forbidden";
const CONVERT_KIND_CHANGE_FORBIDDEN: &str =
    "不可将交易类型改为或改出「转换」：转换的纠错只有「删除后重建」一条路";

const SPLIT_KIND_CHANGE_FORBIDDEN_CODE: &str = "trade.split-kind-change-forbidden";
const SPLIT_KIND_CHANGE_FORBIDDEN: &str =
    "不可将交易类型改为或改出「份额调整」：换交易类型的纠错只有「删除后重建」一条路";

const DIVIDEND_KIND_CHANGE_FORBIDDEN_CODE: &str = "trade.dividend-kind-change-forbidden";
const DIVIDEND_KIND_CHANGE_FORBIDDEN: &str =
    "不可将交易类型改为或改出「分红」：换交易类型的纠错只有「删除后重建」一条路";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteForm {
    Local,
    Replay,
}

#[derive(Debug)]
enum CreateForm<'a> {
    Local(&'a TransactionInput),
    Replay {
        id: &'a str,
        row: &'a NormalizedTransaction,
        investment: Option<&'a InvestmentCommandFields>,
        convert: Option<&'a ConvertCommandFields>,
        split: Option<&'a SplitCommandFields>,
    },
}

#[derive(Debug)]
enum UpdateForm<'a> {
    Local(&'a TransactionInput),
    Replay {
        row: &'a NormalizedTransaction,
        investment: Option<&'a InvestmentCommandFields>,
        convert: Option<&'a ConvertCommandFields>,
        split: Option<&'a SplitCommandFields>,
    },
}

impl UpdateForm<'_> {
    fn kind(&self) -> TransactionKind {
        match self {
            UpdateForm::Local(input) => input.kind,
            UpdateForm::Replay { row, .. } => row.kind,
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum Plan {
    Common(writer::NormalizedRow),
    Investment(Box<dyn InvestmentPlan>),
}

impl Plan {
    fn normalized_row(&self) -> Result<writer::NormalizedRow> {
        match self {
            Plan::Common(row) => Ok(row.clone()),
            Plan::Investment(p) => Ok(writer::NormalizedRow::try_from(p.normalized())?),
        }
    }
}

#[derive(Debug)]
pub struct TransactionWrite {
    pub id: String,
    pub evidence: WriteEvidence,
}

pub fn create(conn: &Connection, input: TransactionInput) -> Result<TransactionWrite> {
    create_protocol(conn, CreateForm::Local(&input))
}

fn create_protocol(conn: &Connection, source: CreateForm<'_>) -> Result<TransactionWrite> {
    ensure_transaction(conn, || {
        if let CreateForm::Local(input) = &source {
            guard_reference_admission(input)?;
        }
        if let CreateForm::Replay { row, .. } = &source {
            writer::validate_accounts_alive(
                conn,
                row.account_id.as_deref(),
                row.to_account_id.as_deref(),
                row.funding_account_id.as_deref(),
                &row.funding
                    .iter()
                    .map(|i| i.account_id.clone())
                    .collect::<Vec<_>>(),
            )?;
        }
        let (plan, merchant_created) = match &source {
            CreateForm::Local(input) => plan(conn, input)?,
            CreateForm::Replay {
                row,
                investment,
                convert,
                split,
                ..
            } => (
                replay_assembly(conn, None, row, *investment, *convert, *split)?,
                false,
            ),
        };
        let row = plan.normalized_row()?;
        let id = match &source {
            CreateForm::Local(_) => writer::insert_row(conn, &row)?,
            CreateForm::Replay { id, .. } => {
                writer::insert_row_with_id(conn, id, &row)?;
                id.to_string()
            }
        };
        apply(conn, &id, &plan)?;
        if let CreateForm::Local(_) = &source {
            record_local(conn, create_command(&id, &plan))?;
        }
        Ok(TransactionWrite {
            id,
            evidence: WriteEvidence::MerchantCreated(merchant_created),
        })
    })
}

pub fn update(conn: &Connection, id: &str, input: TransactionInput) -> Result<WriteEvidence> {
    update_protocol(conn, id, UpdateForm::Local(&input))
}

fn update_protocol(conn: &Connection, id: &str, source: UpdateForm<'_>) -> Result<WriteEvidence> {
    ensure_transaction(conn, || {
        let existing: ExistingRow = conn
            .query_row(
                "SELECT kind, merchant_id, policy_id, amount_cents, currency_code, date, \
                 amount_native_cents, fx_rate_used, fx_rate_source \
                 FROM transactions WHERE id=?1 AND is_deleted=0",
                rusqlite::params![id],
                |r| {
                    Ok(ExistingRow {
                        id: id.to_string(),
                        kind: r.get(0)?,
                        merchant_id: r.get(1)?,
                        policy_id: r.get(2)?,
                        fx_edit_baseline: FxEditBaseline {
                            amount_cents: r.get(3)?,
                            currency_code: r.get(4)?,
                            date: r.get(5)?,
                            conversion: NativeConversion {
                                native_cents: r.get(6)?,
                                fx_rate_used: r.get(7)?,
                                fx_rate_source: r.get(8)?,
                            },
                        },
                    })
                },
            )
            .optional()?
            .ok_or_else(|| {
                AppError::codedp_not_found(
                    "transaction.not-found",
                    format!("交易不存在: {id}"),
                    &[id],
                )
            })?;
        let new_kind = source.kind();
        let old_kind = existing.kind;
        if (old_kind == TransactionKind::Convert) != (new_kind == TransactionKind::Convert) {
            return Err(AppError::coded(
                CONVERT_KIND_CHANGE_FORBIDDEN_CODE,
                CONVERT_KIND_CHANGE_FORBIDDEN,
            ));
        }
        if (old_kind == TransactionKind::Split) != (new_kind == TransactionKind::Split) {
            return Err(AppError::coded(
                SPLIT_KIND_CHANGE_FORBIDDEN_CODE,
                SPLIT_KIND_CHANGE_FORBIDDEN,
            ));
        }
        if (old_kind == TransactionKind::Dividend) != (new_kind == TransactionKind::Dividend) {
            return Err(AppError::coded(
                DIVIDEND_KIND_CHANGE_FORBIDDEN_CODE,
                DIVIDEND_KIND_CHANGE_FORBIDDEN,
            ));
        }
        if let UpdateForm::Local(input) = &source {
            guard_reference_admission(input)?;
        }
        revert_investment(conn, id, old_kind)?;
        if let UpdateForm::Replay { row, .. } = &source {
            writer::validate_accounts_alive(
                conn,
                row.account_id.as_deref(),
                row.to_account_id.as_deref(),
                row.funding_account_id.as_deref(),
                &row.funding
                    .iter()
                    .map(|i| i.account_id.clone())
                    .collect::<Vec<_>>(),
            )?;
        }
        let (plan, merchant_created) = match &source {
            UpdateForm::Local(input) => plan_with_existing_refs(conn, input, Some(&existing))?,
            UpdateForm::Replay {
                row,
                investment,
                convert,
                split,
                ..
            } => (
                replay_assembly(conn, Some(id), row, *investment, *convert, *split)?,
                false,
            ),
        };
        let row = plan.normalized_row()?;
        writer::update_row(conn, id, &row)?;
        apply(conn, id, &plan)?;
        if let UpdateForm::Local(_) = &source {
            record_local(conn, update_command(id, &plan))?;
        }
        Ok(WriteEvidence::MerchantCreated(merchant_created))
    })
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        delete_within_transaction(conn, id, WriteForm::Local)
    })
}

fn delete_within_transaction(conn: &Connection, id: &str, form: WriteForm) -> Result<()> {
    let (kind,): (TransactionKind,) = conn
        .query_row(
            "SELECT kind FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| Ok((r.get(0)?,)),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::codedp_not_found("transaction.not-found", format!("交易不存在: {id}"), &[id])
        })?;

    let cascaded_sell_ids = match kind {
        TransactionKind::Buy
        | TransactionKind::Sell
        | TransactionKind::Convert
        | TransactionKind::Split
        | TransactionKind::Dividend => release_for_delete_investment(conn, id, kind)?,
        _ => Vec::new(),
    };
    for sell_id in &cascaded_sell_ids {
        soft_delete_transaction_row(conn, sell_id, form)?;
    }
    soft_delete_transaction_row(conn, id, form)
}

fn soft_delete_transaction_row(conn: &Connection, id: &str, form: WriteForm) -> Result<()> {
    let (account_id, to_account_id, funding_account_id): (Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT account_id, to_account_id, funding_account_id FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::codedp_not_found("transaction.not-found", format!("交易不存在: {id}"), &[id])
        })?;
    let item_accounts = crate::write::funding_items::item_account_ids(conn, id)?;
    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    let item_refs: Vec<&str> = item_accounts.iter().map(String::as_str).collect();
    crate::seams::balance::refresh_affected_balances(
        conn,
        Some(crate::seams::balance::RowAccounts {
            account_id: account_id.as_deref(),
            to_account_id: to_account_id.as_deref(),
            funding_account_id: funding_account_id.as_deref(),
            funding_items: &item_refs,
        }),
        None,
    )?;
    if form == WriteForm::Local {
        record_local(conn, TransactionCommand::Delete { id: id.to_string() })?;
    }
    Ok(())
}

struct ExistingRow {
    id: String,
    kind: TransactionKind,
    merchant_id: Option<String>,
    policy_id: Option<String>,
    fx_edit_baseline: FxEditBaseline,
}

fn guard_reference_admission(input: &TransactionInput) -> Result<()> {
    let kind = input.kind;
    if (input.merchant_id.is_some() || input.merchant_name.is_some())
        && !matches!(
            kind,
            TransactionKind::Income
                | TransactionKind::Expense
                | TransactionKind::Refund
                | TransactionKind::Transfer
        )
    {
        return Err(AppError::codedp(
            "transaction.merchant-unsupported",
            format!("交易类型 {kind} 不能携带商户"),
            &[&kind.to_string()],
        ));
    }
    if input.category_id.is_some()
        && !matches!(
            kind,
            TransactionKind::Income | TransactionKind::Expense | TransactionKind::Refund
        )
    {
        return Err(AppError::codedp(
            "transaction.category-unsupported",
            format!("交易类型 {kind} 不能携带分类"),
            &[&kind.to_string()],
        ));
    }
    if input.policy_id.is_some()
        && !matches!(kind, TransactionKind::Income | TransactionKind::Expense)
    {
        return Err(AppError::codedp(
            "transaction.policy-unsupported",
            format!("交易类型 {kind} 不能挂保单"),
            &[&kind.to_string()],
        ));
    }
    if !input.funding.is_empty()
        && !matches!(
            kind,
            TransactionKind::Income | TransactionKind::Expense | TransactionKind::Refund
        )
    {
        return Err(AppError::codedp(
            "transaction.funding-item-unsupported",
            format!("交易类型 {kind} 不能携带出资分解"),
            &[&kind.to_string()],
        ));
    }
    if !input.purchases.is_empty() && kind != TransactionKind::Expense {
        return Err(AppError::codedp(
            "transaction.purchase-item-unsupported",
            format!("交易类型 {kind} 不能携带购买项"),
            &[&kind.to_string()],
        ));
    }
    if input.origin.is_some() && !matches!(kind, TransactionKind::Buy) {
        return Err(AppError::codedp(
            "transaction.origin-unsupported",
            format!("交易类型 {kind} 不能携带证券来源口径"),
            &[&kind.to_string()],
        ));
    }
    Ok(())
}

fn plan(conn: &Connection, input: &TransactionInput) -> Result<(Plan, bool)> {
    plan_with_existing_refs(conn, input, None)
}

fn plan_with_existing_refs(
    conn: &Connection,
    input: &TransactionInput,
    existing: Option<&ExistingRow>,
) -> Result<(Plan, bool)> {
    let existing_merchant_id = existing.and_then(|e| e.merchant_id.as_deref());
    let existing_policy_id = existing.and_then(|e| e.policy_id.as_deref());
    let fx_edit_baseline = existing.map(|e| &e.fx_edit_baseline);
    let existing_id = existing.map(|e| e.id.as_str());
    let kind = input.kind;
    match kind {
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => {
            let (merchant_id, pending_name) = if kind == TransactionKind::Refund {
                (None, None)
            } else {
                match (&input.merchant_name, &input.merchant_id) {
                    (Some(_), Some(_)) => {
                        return Err(AppError::coded(
                            "transaction.merchant-id-and-name-conflict",
                            "merchant_id 与 merchant_name 不可同时提供",
                        ));
                    }
                    (Some(name), None) => {
                        match merchant_hooks::find_merchant_by_name(conn, name)? {
                            Some(id) => (Some(id), None),
                            None => (None, Some(name.to_string())),
                        }
                    }
                    (None, id) => (id.clone(), None),
                }
            };
            let mut norm = writer::normalize(
                conn,
                &writer::Input {
                    kind,
                    amount_cents: input.amount_cents,
                    currency_code: input.currency_code.clone(),
                    account_id: input.account_id.clone(),
                    to_account_id: input.to_account_id.clone(),
                    funding_account_id: input.funding_account_id.clone(),
                    funding: input
                        .funding
                        .iter()
                        .map(crate::write::funding_items::FundingItem::from)
                        .collect(),
                    existing_funding: match existing_id {
                        Some(id) => crate::write::funding_items::read_rows(conn, id)?,
                        None => Vec::new(),
                    },
                    purchases: input
                        .purchases
                        .iter()
                        .map(crate::write::purchase_items::PurchaseItem::from)
                        .collect(),
                    existing_purchases: match existing_id {
                        Some(id) => crate::write::purchase_items::read_rows(conn, id)?,
                        None => Vec::new(),
                    },
                    category_id: input.category_id.clone(),
                    merchant_id,
                    existing_merchant_id: existing_merchant_id.map(str::to_string),
                    policy_id: input.policy_id.clone(),
                    existing_policy_id: existing_policy_id.map(str::to_string),
                    refund_of_transaction_id: input.refund_of_transaction_id.clone(),
                    note: input.note.clone(),
                    source_order_no: input.source_order_no.clone(),
                    date: input.date.clone(),
                    fx_rate: input.fx_rate,
                    fx_edit_baseline: fx_edit_baseline.cloned(),
                },
            )?;
            let merchant_created = if let Some(name) = pending_name {
                norm.merchant_id = Some(merchant_hooks::create_merchant_by_name(conn, &name)?);
                true
            } else {
                false
            };
            Ok((Plan::Common(norm), merchant_created))
        }
        TransactionKind::Buy
        | TransactionKind::Sell
        | TransactionKind::Convert
        | TransactionKind::Split
        | TransactionKind::Dividend => Ok((
            Plan::Investment(prepare_investment(
                conn,
                kind,
                input,
                existing_id,
                fx_edit_baseline,
            )?),
            false,
        )),
    }
}

fn apply(conn: &Connection, id: &str, plan: &Plan) -> Result<()> {
    match plan {
        Plan::Common(_) => Ok(()),
        Plan::Investment(p) => p.apply(conn, id),
    }
}

fn command_parts(plan: &Plan) -> (NormalizedTransaction, PlanCommandParts) {
    match plan {
        Plan::Common(r) => (NormalizedTransaction::from(r), PlanCommandParts::none()),
        Plan::Investment(p) => (p.normalized().clone(), p.command_parts()),
    }
}

fn create_command(id: &str, plan: &Plan) -> TransactionCommand {
    let (row, parts) = command_parts(plan);
    TransactionCommand::Create {
        id: id.to_string(),
        row,
        investment: parts.investment,
        split: parts.split,
        convert: parts.convert,
    }
}

fn update_command(id: &str, plan: &Plan) -> TransactionCommand {
    let (row, parts) = command_parts(plan);
    TransactionCommand::Update {
        id: id.to_string(),
        row,
        investment: parts.investment,
        split: parts.split,
        convert: parts.convert,
    }
}

pub fn replay_command(conn: &Connection, command: &TransactionCommand) -> Result<()> {
    match command {
        TransactionCommand::Create {
            id,
            row,
            investment,
            split,
            convert,
        } => {
            create_protocol(
                conn,
                CreateForm::Replay {
                    id,
                    row,
                    investment: investment.as_ref(),
                    convert: convert.as_ref(),
                    split: split.as_ref(),
                },
            )?;
            Ok(())
        }
        TransactionCommand::Update {
            id,
            row,
            investment,
            split,
            convert,
        } => {
            update_protocol(
                conn,
                id,
                UpdateForm::Replay {
                    row,
                    investment: investment.as_ref(),
                    convert: convert.as_ref(),
                    split: split.as_ref(),
                },
            )?;
            Ok(())
        }
        TransactionCommand::Delete { id } => delete_within_transaction(conn, id, WriteForm::Replay),
    }
}

fn replay_assembly(
    conn: &Connection,
    existing_id: Option<&str>,
    row: &NormalizedTransaction,
    investment: Option<&InvestmentCommandFields>,
    convert: Option<&ConvertCommandFields>,
    split: Option<&SplitCommandFields>,
) -> Result<Plan> {
    let norm_row = writer::NormalizedRow::try_from(row)?;
    match norm_row.kind {
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => {
            writer::validate_currency_consistency(
                conn,
                norm_row.account_id.as_deref(),
                norm_row.to_account_id.as_deref(),
                &norm_row.currency_code,
            )?;
            crate::write::funding_items::validate_funding_item_currencies(
                conn,
                &norm_row.funding,
                &norm_row.currency_code,
            )?;
            Ok(Plan::Common(norm_row))
        }
        TransactionKind::Buy
        | TransactionKind::Sell
        | TransactionKind::Convert
        | TransactionKind::Split
        | TransactionKind::Dividend => Ok(Plan::Investment(replay_investment(
            conn,
            row,
            existing_id,
            investment,
            convert,
            split,
        )?)),
    }
}

#[cfg(test)]
mod tests;
