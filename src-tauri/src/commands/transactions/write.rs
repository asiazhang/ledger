use rusqlite::Connection;
use rusqlite::OptionalExtension;

use crate::error::{AppError, Result};
use crate::models::{NormalizedTransaction, TransactionInput};
use crate::transaction::amount;
use crate::transaction::writer;

pub fn insert_transaction(conn: &Connection, input: TransactionInput) -> Result<String> {
    let id = if input.kind == "buy" {
        crate::commands::investment::create_buy_transaction(conn, input)?
    } else if input.kind == "sell" {
        crate::commands::investment::create_sell_transaction(conn, input)?
    } else {
        // 通用 kind（income/expense/transfer/refund）经 Writer 接缝归一化并落库
        // （issue #60 / spec #52）：本位币折算走 Amount 接缝（全局默认币种基准），
        // id 与审计字段由 writer::insert_row 统一生成，与定时引擎/批量导入共用
        // 同一写入权威，列清单不在此重复。
        let norm = writer::normalize(conn, &to_writer_input(&input)?)?;
        writer::insert_row(conn, &norm)?
    };
    // 索引维护由后台定时刷新（ADR-0004 决策 #14）承担：触发器已入队
    // `search_reindex_queue`，写路径不做任何同步索引工作（界面操作零索引开销）。
    Ok(id)
}

/// 校验并按 kind 归一化交易字段，产出可直接 INSERT/UPDATE 的交易行字段。
///
/// 命令层归一化入口：buy/sell 委托投资层 `prepare_buy`/`prepare_sell`（投资字段校验 + 归一化行产出），
/// 通用 kind（income/expense/transfer/refund）委托 Writer 接缝 [`writer::normalize`]（issue #60）：
/// 校验（转账目标账户/金额>0/退款继承）与本位币折算口径与落库路径统一
/// （Amount 接缝，全局默认币种基准）。
///
/// 注意：创建/修改热路径已直接调用 [`writer::normalize`]（见 `insert_transaction` /
/// `update_transaction_internal`），本函数保留 buy/sell + 通用 kind 的统一入口语义供测试锁定，
/// 旧命令层实现（账户币种折算 + 裸 SQL）已随接线被取代，随 issue #61 删除。
pub fn normalize_transaction(
    conn: &Connection,
    input: &TransactionInput,
) -> Result<NormalizedTransaction> {
    match input.kind.as_str() {
        "buy" => crate::commands::investment::prepare_buy(conn, input).map(|p| p.normalized),
        "sell" => crate::commands::investment::prepare_sell(conn, input).map(|p| p.normalized),
        // 通用 kind（income/expense/transfer/refund）委托 Writer 接缝（issue #60）：
        // 校验（转账目标账户/金额>0/退款继承）与本位币折算口径与落库路径统一
        // （Amount 接缝，全局默认币种基准）。旧命令层实现（账户币种折算）已随接线
        // 被取代，随 issue #61 删除。
        _ => Ok(row_to_normalized(writer::normalize(
            conn,
            &to_writer_input(input)?,
        )?)),
    }
}

/// `TransactionInput` → `writer::Input`（命令层接线转换：丢弃投资与幂等字段）。
fn to_writer_input(input: &TransactionInput) -> Result<writer::Input> {
    Ok(writer::Input {
        kind: amount::Kind::parse(&input.kind)?,
        amount_cents: input.amount_cents,
        currency_code: input.currency_code.clone(),
        account_id: input.account_id.clone(),
        to_account_id: input.to_account_id.clone(),
        category_id: input.category_id.clone(),
        refund_of_transaction_id: input.refund_of_transaction_id.clone(),
        note: input.note.clone(),
        date: input.date.clone(),
    })
}

/// `NormalizedTransaction` → `writer::NormalizedRow`（命令层接线转换）。
///
/// buy/sell 归一化行同样经此转换后走 [`writer::insert_row`]/[`writer::update_row`]
/// 落交易行字段（其持仓/卖出关联副作用由投资层另行处理）。
pub(crate) fn to_writer_row(norm: &NormalizedTransaction) -> Result<writer::NormalizedRow> {
    Ok(writer::NormalizedRow {
        kind: amount::Kind::parse(&norm.kind)?,
        amount_cents: norm.amount_cents,
        currency_code: norm.currency_code.clone(),
        amount_native_cents: norm.amount_native_cents,
        account_id: norm.account_id.clone(),
        to_account_id: norm.to_account_id.clone(),
        category_id: norm.category_id.clone(),
        refund_of_transaction_id: norm.refund_of_transaction_id.clone(),
        note: norm.note.clone(),
        date: norm.date.clone(),
    })
}

/// `writer::NormalizedRow` → 命令层 `NormalizedTransaction`（接线转换：kind 转回字符串）。
fn row_to_normalized(row: writer::NormalizedRow) -> NormalizedTransaction {
    NormalizedTransaction {
        kind: row.kind.as_str().to_string(),
        amount_cents: row.amount_cents,
        currency_code: row.currency_code,
        amount_native_cents: row.amount_native_cents,
        account_id: row.account_id,
        to_account_id: row.to_account_id,
        category_id: row.category_id,
        refund_of_transaction_id: row.refund_of_transaction_id,
        note: row.note,
        date: row.date,
    }
}

/// 按 `id` 全字段替换一笔交易（`PUT /api/v1/transactions/{id}`）。
///
/// 通用 kind（income/expense/transfer/refund）经 Writer 接缝 [`writer::normalize`] +
/// [`writer::update_row`] 校验并归一化字段（issue #60），关联约束与创建路径一致；
/// buy/sell 复用 `prepare_buy`/`prepare_sell`（经 `apply_buy`/`apply_sell`）。
/// 幂等键（`idempotency_key`）与内容哈希（`dedup_hash`）不作为
/// 可编辑字段——修改不重算去重身份，故修改后重跑同批导入（带幂等键）仍按同键去重、不产生重复。
///
/// buy/sell 的持仓/卖出关联在各自替换路径处理（先按旧 kind 清理/回补，再按新 kind 重建），
/// 跨 kind 修改（如 expense→buy）避免孤儿持仓。整笔修改在事务内完成，校验或匹配失败回滚。
/// 不存在或已软删除的 id 返回 `AppError::NotFound`。
pub fn update_transaction_internal(
    conn: &Connection,
    id: &str,
    input: TransactionInput,
) -> Result<()> {
    // 读取旧交易 kind，用于按旧 kind 清理持仓/卖出关联；不存在或已删除返回 NotFound。
    let old_kind: String = conn
        .query_row(
            "SELECT kind FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("交易不存在: {id}")))?;

    conn.execute("BEGIN", [])?;
    let res = (|| -> Result<()> {
        // 先按旧 kind 清理/回补持仓副作用，再按新 kind 校验并应用（跨 kind 修改避免孤儿持仓）。
        match old_kind.as_str() {
            "buy" => crate::commands::investment::cleanup_buy(conn, id)?,
            "sell" => crate::commands::investment::reverse_sell(conn, id)?,
            _ => {}
        }
        match input.kind.as_str() {
            "buy" => crate::commands::investment::apply_buy(conn, id, &input),
            "sell" => crate::commands::investment::apply_sell(conn, id, &input),
            _ => {
                // 通用 kind 经 Writer 接缝归一化并更新（issue #60）：校验/折算口径与
                // 创建路径统一；created_at 与幂等身份（idempotency_key/dedup_hash）由
                // update_row 保留，version 递增。
                let norm = writer::normalize(conn, &to_writer_input(&input)?)?;
                writer::update_row(conn, id, &norm)?;
                Ok(())
            }
        }
    })();
    match res {
        Ok(()) => {
            conn.execute("COMMIT", [])?;
            Ok(())
        }
        Err(e) => {
            conn.execute("ROLLBACK", [])?;
            Err(e)
        }
    }
}
