use rusqlite::Connection;

use crate::commands::fx::account_currency_code;
use crate::db::query::{FromRow, query_all};
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{AccountType, NormalizedTransaction, TransactionInput};
use crate::transaction::{amount, writer};

/// 交易行落库（INSERT 侧）的统一入口：全部经 [`writer::insert_row`]。
///
/// 本文件内 `write_buy` / `write_sell` / `apply_buy` / `apply_sell` 均经此函数
/// 落交易行字段，不再各自拼写 INSERT/UPDATE 列清单（issue #70：交易行写入唯一权威）；
/// 归一化行 → writer 行的转换随 `models::NormalizedTransaction` 定义（TryFrom），
/// investment 不再反向依赖 transactions 模块的行更新函数（双向依赖斩断）。
fn write_txn_row(conn: &Connection, normalized: &NormalizedTransaction) -> Result<String> {
    writer::insert_row(conn, &writer::NormalizedRow::try_from(normalized)?)
}

/// 交易行更新（UPDATE 侧）的统一入口：全部经 [`writer::update_row`]。
///
/// 保留 `created_at` 与幂等身份，`version` 递增；持仓/卖出关联副作用由调用方另行处理。
fn update_txn_row(conn: &Connection, id: &str, normalized: &NormalizedTransaction) -> Result<()> {
    writer::update_row(conn, id, &writer::NormalizedRow::try_from(normalized)?)
}

pub(crate) struct ActiveLot {
    pub(crate) id: String,
    pub(crate) remaining_quantity: f64,
    pub(crate) cost_per_unit_cents: i64,
    pub(crate) currency_code: String,
}

impl FromRow for ActiveLot {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(ActiveLot {
            id: row.get(0)?,
            remaining_quantity: row.get(1)?,
            cost_per_unit_cents: row.get(2)?,
            currency_code: row.get(3)?,
        })
    }
}

pub(crate) struct BuyPlan {
    pub(crate) normalized: NormalizedTransaction,
    pub(crate) instrument_id: String,
    pub(crate) quantity: f64,
    pub(crate) price_cents: i64,
    pub(crate) fee_cents: i64,
}

/// 校验并归一化一笔买入交易（不落库）。创建与修改共用；
/// 只做校验与字段解析，持仓建仓等副作用由调用方在落库时按其身份（新增或替换）执行。
pub(crate) fn prepare_buy(conn: &Connection, input: &TransactionInput) -> Result<BuyPlan> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| AppError::Invalid("买入必须指定标的".into()))?
        .clone();
    let quantity = input.quantity.unwrap_or(0.0);
    let price_cents = input.price_cents.unwrap_or(0);
    let fee_cents = input.fee_cents.unwrap_or(0);
    if quantity <= 0.0 {
        return Err(AppError::Invalid("买入数量必须大于 0".into()));
    }
    if price_cents <= 0 {
        return Err(AppError::Invalid("买入单价必须大于 0".into()));
    }
    let account_type: AccountType = conn
        .query_row(
            "SELECT type FROM accounts WHERE id=?1",
            rusqlite::params![input.account_id],
            |r| r.get::<_, String>(0),
        )?
        .parse()?;
    if account_type != AccountType::Investment {
        return Err(AppError::Invalid("买入交易必须使用投资账户".into()));
    }
    let account_currency = account_currency_code(conn, &input.account_id)?;
    let amount_cents = (quantity * price_cents as f64).round() as i64 + fee_cents;
    // 本位币金额经 Amount 接缝折算到全局默认币种（issue #70）：不再硬编码 1:1，
    // 与通用 kind / 定时引擎共用同一折算路径（convert_to_native，基准为默认币种）。
    let amount_native_cents = amount::convert_to_native(conn, amount_cents, &account_currency)?;

    Ok(BuyPlan {
        normalized: NormalizedTransaction {
            kind: "buy".into(),
            amount_cents,
            currency_code: account_currency,
            amount_native_cents,
            account_id: input.account_id.clone(),
            to_account_id: input.to_account_id.clone(),
            category_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            date: input.date.clone(),
        },
        instrument_id,
        quantity,
        price_cents,
        fee_cents,
    })
}

pub(crate) fn create_buy_transaction(conn: &Connection, input: TransactionInput) -> Result<String> {
    let plan = prepare_buy(conn, &input)?;
    write_buy(conn, &plan)
}

/// 落交易行字段经 Writer 接缝（issue #60 / spec #52）：id 与审计字段由
/// [`writer::insert_row`] 统一生成，与通用 kind 创建共用同一写入权威；
/// 持仓建仓（`security_lots` / `security_transactions`）留在命令层按 buy 身份执行。
fn write_buy(conn: &Connection, plan: &BuyPlan) -> Result<String> {
    let id = write_txn_row(conn, &plan.normalized)?;
    create_buy_lot(
        conn,
        &id,
        &plan.normalized.account_id,
        &plan.instrument_id,
        plan.quantity,
        plan.price_cents,
        plan.fee_cents,
        &plan.normalized.currency_code,
    )?;
    Ok(id)
}

/// 校验并归一化一笔卖出交易（不落库）。创建与修改共用；
/// 卖出匹配持仓等副作用由调用方在落库时按其身份执行。
pub(crate) fn prepare_sell(conn: &Connection, input: &TransactionInput) -> Result<SellPlan> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| AppError::Invalid("卖出必须指定标的".into()))?
        .clone();
    let quantity = input.quantity.unwrap_or(0.0);
    let price_cents = input.price_cents.unwrap_or(0);
    let fee_cents = input.fee_cents.unwrap_or(0);
    if quantity <= 0.0 {
        return Err(AppError::Invalid("卖出数量必须大于 0".into()));
    }
    if price_cents <= 0 {
        return Err(AppError::Invalid("卖出单价必须大于 0".into()));
    }
    let account_type: AccountType = conn
        .query_row(
            "SELECT type FROM accounts WHERE id=?1",
            rusqlite::params![input.account_id],
            |r| r.get::<_, String>(0),
        )?
        .parse()?;
    if account_type != AccountType::Investment {
        return Err(AppError::Invalid("卖出交易必须使用投资账户".into()));
    }
    let account_currency = account_currency_code(conn, &input.account_id)?;
    let gross_proceeds = (quantity * price_cents as f64).round() as i64;
    if fee_cents > gross_proceeds {
        return Err(AppError::Invalid("卖出手续费不能超过卖出收入".into()));
    }
    let amount_cents = gross_proceeds - fee_cents;
    // 本位币金额经 Amount 接缝折算到全局默认币种（issue #70）：不再硬编码 1:1，
    // 与通用 kind / 定时引擎共用同一折算路径（convert_to_native，基准为默认币种）。
    let amount_native_cents = amount::convert_to_native(conn, amount_cents, &account_currency)?;

    let lots: Vec<ActiveLot> = query_all(
        conn,
        "SELECT id, remaining_quantity, cost_per_unit_cents, currency_code \
         FROM security_lots \
         WHERE account_id=?1 AND instrument_id=?2 AND remaining_quantity > 0 \
         ORDER BY created_at ASC, id ASC",
        rusqlite::params![input.account_id, instrument_id],
    )?;
    let total_available: f64 = lots.iter().map(|l| l.remaining_quantity).sum();
    if total_available < quantity {
        return Err(AppError::Invalid(format!(
            "可卖出数量不足，当前持有 {}，尝试卖出 {}",
            total_available, quantity
        )));
    }

    Ok(SellPlan {
        normalized: NormalizedTransaction {
            kind: "sell".into(),
            amount_cents,
            currency_code: account_currency,
            amount_native_cents,
            account_id: input.account_id.clone(),
            to_account_id: input.to_account_id.clone(),
            category_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            date: input.date.clone(),
        },
        instrument_id,
        quantity,
        price_cents,
        fee_cents,
        lots,
    })
}

pub(crate) fn create_sell_transaction(
    conn: &Connection,
    input: TransactionInput,
) -> Result<String> {
    let plan = prepare_sell(conn, &input)?;
    write_sell(conn, &plan)
}

/// 落交易行字段经 Writer 接缝（issue #60 / spec #52）：id 与审计字段由
/// [`writer::insert_row`] 统一生成；卖出匹配/持仓扣减副作用留在命令层。
fn write_sell(conn: &Connection, plan: &SellPlan) -> Result<String> {
    let id = write_txn_row(conn, &plan.normalized)?;
    write_sell_side_effects(conn, &id, plan)?;
    Ok(id)
}

/// 卖出交易的持仓/卖出关联副作用（创建与修改共用）。
///
/// 只写 `security_transactions` 记录、`security_lot_sales` 匹配与持仓扣减，不写交易行——
/// 修改路径先 `reverse_sell` 清空旧卖出再由本函数按新输入重建，创建路径在插入交易行后复用。
pub(crate) fn write_sell_side_effects(conn: &Connection, id: &str, plan: &SellPlan) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'sell',?3,?4,?5)",
        rusqlite::params![id, plan.instrument_id, plan.quantity, plan.price_cents, plan.fee_cents],
    )?;

    let mut remaining_to_sell = plan.quantity;
    let mut matched_lots: Vec<ActiveLot> = Vec::new();
    for lot in &plan.lots {
        if remaining_to_sell <= 0.0 {
            break;
        }
        let matched = lot.remaining_quantity.min(remaining_to_sell);
        matched_lots.push(ActiveLot {
            id: lot.id.clone(),
            remaining_quantity: matched,
            cost_per_unit_cents: lot.cost_per_unit_cents,
            currency_code: lot.currency_code.clone(),
        });
        remaining_to_sell -= matched;
    }

    let match_count = matched_lots.len();
    let mut allocated_fee_total = 0i64;
    for (i, lot) in matched_lots.iter().enumerate() {
        let lot_proceeds = (lot.remaining_quantity * plan.price_cents as f64).round() as i64;
        let lot_cost = (lot.remaining_quantity * lot.cost_per_unit_cents as f64).round() as i64;
        let allocated_fee = if i == match_count - 1 {
            plan.fee_cents - allocated_fee_total
        } else {
            let fee =
                (plan.fee_cents as f64 * lot.remaining_quantity / plan.quantity).floor() as i64;
            allocated_fee_total += fee;
            fee
        };
        let realized_pnl = lot_proceeds - lot_cost - allocated_fee;
        let sale_id = new_uuid();
        conn.execute(
            "INSERT INTO security_lot_sales (id,sell_transaction_id,lot_id,quantity,cost_per_unit_cents,realized_pnl_cents,currency_code,created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![sale_id, id, lot.id, lot.remaining_quantity, lot.cost_per_unit_cents, realized_pnl, lot.currency_code, now],
        )?;
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity-?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![lot.remaining_quantity, now, device_id(), lot.id],
        )?;
    }

    Ok(())
}

/// 按 id 替换一笔买入交易：先清旧持仓关联，再按新输入重建（创建与修改共用校验）。
///
/// 由 `update_transaction_internal` 在事务内调用：若该买入已有部分卖出，其持仓不可安全替换
/// （会破坏对应卖出的已实现盈亏），返回 `Invalid`。
pub(crate) fn apply_buy(conn: &Connection, id: &str, input: &TransactionInput) -> Result<()> {
    let plan = prepare_buy(conn, input)?;
    // 交易行字段经 Writer 接缝更新（issue #60）：保留 created_at 与幂等身份，
    // version 递增；持仓重建留在命令层。
    update_txn_row(conn, id, &plan.normalized)?;
    create_buy_lot(
        conn,
        id,
        &plan.normalized.account_id,
        &plan.instrument_id,
        plan.quantity,
        plan.price_cents,
        plan.fee_cents,
        &plan.normalized.currency_code,
    )?;
    Ok(())
}

/// 清理一笔买入交易的持仓关联（软删除与按 id 修改共用的守卫 + 清理）。
///
/// 若该买入已有部分卖出（`remaining_quantity < initial_quantity`）则拒绝清理——避免破坏
/// 对应卖出的已实现盈亏。`partially_sold_msg` 用于区分「删除」/「修改」场景的错误措辞。
pub(crate) fn cleanup_buy_side_effects(
    conn: &Connection,
    id: &str,
    partially_sold_msg: &str,
) -> Result<()> {
    let partially_sold: i64 = conn.query_row(
        "SELECT COUNT(*) FROM security_lots \
         WHERE buy_transaction_id=?1 AND remaining_quantity < initial_quantity",
        rusqlite::params![id],
        |r| r.get(0),
    )?;
    if partially_sold > 0 {
        return Err(AppError::Invalid(partially_sold_msg.into()));
    }
    conn.execute(
        "DELETE FROM security_lots WHERE buy_transaction_id=?1",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_transactions WHERE transaction_id=?1 AND action='buy'",
        rusqlite::params![id],
    )?;
    Ok(())
}

/// 按 id 修改买入时的持仓清理，复用 `cleanup_buy_side_effects` 的守卫与清理。
pub(crate) fn cleanup_buy(conn: &Connection, id: &str) -> Result<()> {
    cleanup_buy_side_effects(conn, id, "该买入交易已有部分卖出，无法修改")
}

/// 按 id 替换一笔卖出交易：先回补旧卖出的持仓扣减，再按新输入重新匹配。
///
/// 由 `update_transaction_internal` 在事务内调用：回补后使修改可从当前持仓状态重新校验，
/// 校验或匹配失败时由外层回滚整体还原。
pub(crate) fn apply_sell(conn: &Connection, id: &str, input: &TransactionInput) -> Result<()> {
    let plan = prepare_sell(conn, input)?;
    // 交易行字段经 Writer 接缝更新（issue #60）：保留 created_at 与幂等身份，
    // version 递增；卖出匹配/持仓扣减重建留在命令层。
    update_txn_row(conn, id, &plan.normalized)?;
    write_sell_side_effects(conn, id, &plan)?;
    Ok(())
}

/// 回补一笔卖出交易曾扣减的持仓并按新输入重建的逆向操作：把每笔 `security_lot_sales` 的数量
/// 加回对应 lot，再清空该卖出的 `security_lot_sales` 与 `security_transactions` 记录。
pub(crate) fn reverse_sell(conn: &Connection, id: &str) -> Result<()> {
    let now = now_iso();
    let mut stmt = conn
        .prepare("SELECT lot_id, quantity FROM security_lot_sales WHERE sell_transaction_id=?1")?;
    let sales: Vec<(String, f64)> = stmt
        .query_map(rusqlite::params![id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    drop(stmt);
    for (lot_id, quantity) in sales {
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity+?1, \
             updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![quantity, now, device_id(), lot_id],
        )?;
    }
    conn.execute(
        "DELETE FROM security_lot_sales WHERE sell_transaction_id=?1",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_transactions WHERE transaction_id=?1 AND action='sell'",
        rusqlite::params![id],
    )?;
    Ok(())
}

pub(crate) struct SellPlan {
    pub(crate) normalized: NormalizedTransaction,
    pub(crate) instrument_id: String,
    pub(crate) quantity: f64,
    pub(crate) price_cents: i64,
    pub(crate) fee_cents: i64,
    pub(crate) lots: Vec<ActiveLot>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn create_buy_lot(
    conn: &Connection,
    transaction_id: &str,
    account_id: &str,
    instrument_id: &str,
    quantity: f64,
    price_cents: i64,
    fee_cents: i64,
    currency_code: &str,
) -> Result<()> {
    let lot_id = new_uuid();
    let now = now_iso();
    let total_cost_cents = (quantity * price_cents as f64).round() as i64 + fee_cents;
    let cost_per_unit = if quantity > 0.0 {
        (total_cost_cents as f64 / quantity).round() as i64
    } else {
        0
    };
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',?3,?4,?5)",
        rusqlite::params![transaction_id, instrument_id, quantity, price_cents, fee_cents],
    )?;
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?5,?6,?7,?8,?8,?9,?10)",
        rusqlite::params![
            lot_id,
            account_id,
            instrument_id,
            transaction_id,
            quantity,
            cost_per_unit,
            currency_code,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(())
}
