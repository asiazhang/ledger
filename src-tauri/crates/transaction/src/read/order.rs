//! 订单汇总读回（读路径，issue #1862 / ADR-0138 决策 9）：按来源订单号取同单
//! 行集与出资聚合的只读单点。
//!
//! 职责：[`get_transaction_order_summary`]——同单未删除行集（日期升序，明细与
//! 出资项 attach 契约与列表同形）、行数、合计与按账户聚合的出资构成。不变量：
//! 行集 SELECT 与出资子行 SELECT 是多语句读闭包，经 `ensure_transaction` 收同一
//! 读事务（#1699 同款纪律，配读快照探针测试，删除接线即红）。ADR 指针：
//! ADR-0138 决策 9。陷阱：软删行一律排除（行集与聚合同口径）；空单（全部行被
//! 删除）返回零行汇总、不报 NotFound（前端按空态渲染）。

use rusqlite::Connection;
use rusqlite::params;

use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;

use crate::model::{OrderAccountContribution, TransactionOrderSummary};
use crate::read::funding::attach_fundings;
use crate::read::purchase::attach_purchases;

/// 按来源订单号读取订单汇总（详情订单区只读命令的行为权威）。
///
/// 行序为日期升序（`date ASC, created_at ASC, id ASC`，id 最终 tiebreaker）——
/// 对照回单核对的自然阅读序，与列表的倒序展示互补。合计是各行原始币种金额的
/// 直和；出资构成按账户聚合（分解行逐出资项、单出资行按主账户整额，口径见
/// [`TransactionOrderSummary`] 注记），聚合序随行序与出资顺序位、同账户合并。
pub fn get_transaction_order_summary(
    conn: &Connection,
    source_order_no: &str,
) -> Result<TransactionOrderSummary> {
    // 读快照一致性（issue #1699 同款纪律）：行集与出资子行是多语句读闭包——写
    // 提交落在两语句之间会读到「行集为旧、出资为新」的混搭口径（出资构成之和
    // ≠ 行集合计）。整体收进同一读事务（嵌套感知），配读快照探针测试。
    ensure_transaction(conn, || {
        let mut items = query_all::<crate::model::Transaction, _>(
            conn,
            "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
         to_account_id,funding_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,\
         version,device_id,is_deleted,merchant_id,policy_id,fx_rate_used,fx_rate_source,source_order_no \
         FROM transactions WHERE source_order_no=?1 AND is_deleted=0 \
         ORDER BY date ASC, created_at ASC, id ASC",
            params![source_order_no],
        )?;
        // 出资项读闭包与行集同一读事务：分解行明细与派生分解的读数都锚在行集
        // 快照上（attach_fundings 只读，事务归属由本闭包保证）。
        attach_fundings(conn, &mut items)?;
        // 购买项读闭包与行集同一读事务（issue #1882 / ADR-0138 影响节）：明细读数
        // 锚在行集快照上（attach_purchases 只读，事务归属由本闭包保证）。
        attach_purchases(conn, &mut items)?;

        let row_count = items.len() as i64;
        let total_amount_cents: i64 = items.iter().map(|t| t.amount_cents).sum();
        // 币种唯一才携带：单回单订单的常态；混合币种 None，不静默混算。
        let mut currencies = items.iter().map(|t| t.currency_code.as_str());
        let currency_code = match currencies.next() {
            Some(first) => (currencies.all(|c| c == first)).then(|| first.to_string()),
            None => None,
        };

        // 出资构成按账户聚合：聚合序随行序与行内出资顺序位（稳定、可复现），
        // 同账户多条合并；单出资行按主账户整额计入，其余端不计（口径见模型注记）。
        let mut accounts: Vec<OrderAccountContribution> = Vec::new();
        let index_of = |accounts: &Vec<OrderAccountContribution>, id: &str| {
            accounts.iter().position(|a| a.account_id == id)
        };
        for t in items.iter() {
            if !t.fundings.is_empty() {
                for f in t.fundings.iter() {
                    match index_of(&accounts, &f.account_id) {
                        Some(i) => accounts[i].amount_cents += f.amount_cents,
                        None => accounts.push(OrderAccountContribution {
                            account_id: f.account_id.clone(),
                            amount_cents: f.amount_cents,
                        }),
                    }
                }
            } else if let Some(account_id) = t.account_id.as_deref() {
                match index_of(&accounts, account_id) {
                    Some(i) => accounts[i].amount_cents += t.amount_cents,
                    None => accounts.push(OrderAccountContribution {
                        account_id: account_id.to_string(),
                        amount_cents: t.amount_cents,
                    }),
                }
            }
        }

        Ok(TransactionOrderSummary {
            source_order_no: source_order_no.to_string(),
            row_count,
            total_amount_cents,
            currency_code,
            accounts,
            items,
        })
    })
}
