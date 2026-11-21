use std::collections::HashMap;

use rusqlite::Connection;

use super::source as instrument_source;
use super::trade::{self, Plan, replay_convert_plan, replay_plan, replay_split_plan};
use ledger_infra::error::{AppError, Result};
use ledger_transaction::amount::FxEditBaseline;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::command::{
    ConvertCommandFields, InvestmentCommandFields, SplitCommandFields,
};
use ledger_transaction::seams::investment::{
    ConvertFieldsResolver, InstrumentSourceResolver, PlanCommandParts, PrepareHook,
    ReleaseForDeleteHook, ReplayAssemblyHook, RevertHook,
};
use ledger_transaction::seams::investment::{
    register_convert_fields_resolver, register_instrument_source_resolver, register_prepare_hook,
    register_release_for_delete_hook, register_replay_hook, register_revert_hook,
};
use ledger_transaction::{
    ConvertFields, NormalizedTransaction, TransactionInput, TransactionSource,
    TransactionSourceKind,
};

impl ledger_transaction::seams::investment::InvestmentPlan for Plan {
    fn normalized(&self) -> &NormalizedTransaction {
        Plan::normalized(self)
    }

    fn command_parts(&self) -> PlanCommandParts {
        let fields = match self {
            Plan::Buy(b) => Some(InvestmentCommandFields {
                instrument_id: b.instrument_id.clone(),
                quantity: b.quantity,
                price_cents: b.price_cents,
                fee_cents: b.fee_cents,
                cost_per_unit_cents: Some(b.cost_per_unit_cents),
                origin: Some(b.origin),
            }),
            Plan::Sell(s) => Some(InvestmentCommandFields {
                instrument_id: s.instrument_id.clone(),
                quantity: s.quantity,
                price_cents: s.price_cents,
                fee_cents: s.fee_cents,
                cost_per_unit_cents: None,
                origin: None,
            }),
            Plan::Dividend(d) => Some(InvestmentCommandFields {
                instrument_id: d.instrument_id.clone(),
                quantity: 0.0,
                price_cents: 0,
                fee_cents: 0,
                cost_per_unit_cents: None,
                origin: None,
            }),
            Plan::Split(_) | Plan::Convert(_) => None,
        };
        let split = match self {
            Plan::Split(s) => Some(SplitCommandFields {
                instrument_id: s.instrument_id.clone(),
                delta_quantity: s.delta_quantity,
                final_quantity: s.final_quantity,
                total_cost_cents: s.total_cost_cents,
            }),
            Plan::Buy(_) | Plan::Sell(_) | Plan::Dividend(_) | Plan::Convert(_) => None,
        };
        let convert = match self {
            Plan::Convert(c) => Some(ConvertCommandFields {
                instrument_id: c.instrument_id.clone(),
                quantity: c.quantity,
                to_instrument_id: c.to_instrument_id.clone(),
                to_quantity: c.to_quantity,
                out_amount_cents: c.out_amount_cents,
                in_amount_cents: c.in_amount_cents,
                fee_cents: c.fee_cents,
                carried_cost_cents: c.carried_cost_cents(),
            }),
            Plan::Buy(_) | Plan::Sell(_) | Plan::Dividend(_) | Plan::Split(_) => None,
        };
        PlanCommandParts {
            investment: fields,
            split,
            convert,
        }
    }

    fn apply(&self, conn: &Connection, transaction_id: &str) -> Result<()> {
        trade::apply(conn, transaction_id, self)
    }
}

fn prepare_hook(
    conn: &Connection,
    kind: TransactionKind,
    input: &TransactionInput,
    existing_id: Option<&str>,
    fx_edit_baseline: Option<&FxEditBaseline>,
) -> Result<Box<dyn ledger_transaction::seams::investment::InvestmentPlan>> {
    Ok(Box::new(trade::prepare(
        conn,
        kind,
        input,
        existing_id,
        fx_edit_baseline,
    )?))
}

fn investment_fields(
    investment: Option<&InvestmentCommandFields>,
) -> Result<&InvestmentCommandFields> {
    investment.ok_or_else(|| AppError::Invalid("投资命令缺少投资字段（程序缺陷）".into()))
}

fn convert_fields(convert: Option<&ConvertCommandFields>) -> Result<&ConvertCommandFields> {
    convert.ok_or_else(|| {
        AppError::coded(
            "transaction.convert-fields-missing",
            "该转换操作缺少同步所需的转换字段（产生自较早版本），无法在本机重放；请在来源设备上删除并重新录入该转换后再次同步",
        )
    })
}

fn split_fields(split: Option<&SplitCommandFields>) -> Result<&SplitCommandFields> {
    split.ok_or_else(|| {
        AppError::coded(
            "transaction.split-fields-missing",
            "该份额调整操作缺少同步所需的份额调整字段（产生自较早版本），无法在本机重放；请在来源设备上删除并重新录入该份额调整后再次同步",
        )
    })
}

fn replay_hook(
    conn: &Connection,
    row: &NormalizedTransaction,
    existing_id: Option<&str>,
    investment: Option<&InvestmentCommandFields>,
    convert: Option<&ConvertCommandFields>,
    split: Option<&SplitCommandFields>,
) -> Result<Box<dyn ledger_transaction::seams::investment::InvestmentPlan>> {
    match row.kind {
        TransactionKind::Buy | TransactionKind::Sell | TransactionKind::Dividend => {
            let fields = investment_fields(investment)?;
            Ok(Box::new(replay_plan(conn, row.kind, row, fields)?))
        }
        TransactionKind::Convert => {
            let fields = convert_fields(convert)?;
            Ok(Box::new(replay_convert_plan(conn, row, fields)?))
        }
        TransactionKind::Split => {
            let fields = split_fields(split)?;
            Ok(Box::new(replay_split_plan(conn, row, fields, existing_id)?))
        }
        kind @ (TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund) => Err(AppError::Invalid(format!(
            "投资层仅处理 buy/sell/convert/split/dividend，收到: {kind}"
        ))),
    }
}

fn instrument_source_resolver(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, TransactionSource>> {
    let rows = instrument_source::source_display_by_transaction_ids(conn, transaction_ids)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.transaction_id.clone(),
                TransactionSource {
                    kind: TransactionSourceKind::Instrument,
                    entity_id: row.instrument_id.clone(),
                    display_name: row.display_label(),
                    status: None,
                },
            )
        })
        .collect())
}

fn convert_fields_resolver(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, ConvertFields>> {
    Ok(
        trade::convert_fields_by_transaction_ids(conn, transaction_ids)?
            .into_iter()
            .collect(),
    )
}

pub fn install_transaction_hooks() {
    let prepare: PrepareHook = prepare_hook;
    let replay: ReplayAssemblyHook = replay_hook;
    let revert: RevertHook = trade::revert;
    let release: ReleaseForDeleteHook = trade::release_for_delete;
    register_prepare_hook(prepare);
    register_replay_hook(replay);
    register_revert_hook(revert);
    register_release_for_delete_hook(release);
    let instrument: InstrumentSourceResolver = instrument_source_resolver;
    let convert: ConvertFieldsResolver = convert_fields_resolver;
    register_instrument_source_resolver(instrument);
    register_convert_fields_resolver(convert);
}
