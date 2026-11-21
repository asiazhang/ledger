use cucumber::gherkin::Step;
use cucumber::{then, when};
use rusqlite::params;

use ledger_investment::{InstrumentInput, InstrumentType, create_instrument};
use ledger_transaction::TransactionBatch;
use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;

use crate::common::instrument_id_by_symbol;
use crate::step_inputs::trade_input;
use crate::world::LedgerWorld;

#[when(expr = "幂等创建标的 {string} 类型 {string} 名称 {string} 市场 {string} 币种 {string}")]
fn create_instrument_idempotently(
    world: &mut LedgerWorld,
    symbol: String,
    kind: String,
    name: String,
    market: String,
    currency: String,
) {
    let kind = InstrumentType::parse(&kind).expect("未知金融工具类型");
    create_instrument(
        &world_conn!(world),
        InstrumentInput {
            symbol,
            kind,
            name: Some(name),
            currency_code: currency,
            market: Some(market),
        },
    )
    .expect("幂等创建标的失败");
}

#[when(expr = "批量导入投资交易")]
fn batch_import_trades(world: &mut LedgerWorld, #[step] step: &Step) {
    let table = step.table.as_ref().expect("批量导入投资交易步骤缺少数据表");
    let headers = &table.rows[0];
    let col = |name: &str| headers.iter().position(|h| h == name);
    let get = |row: &[String], name: &str| {
        col(name)
            .and_then(|i| row.get(i).cloned())
            .unwrap_or_default()
    };

    let mut inputs: Vec<TransactionInput> = Vec::new();
    for row in table.rows.iter().skip(1) {
        let symbol = get(row, "标的");
        let account_name = get(row, "账户");
        let kind = get(row, "kind");
        let kind =
            TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）"));
        let (instrument_id, account_id, currency_code) = {
            let conn = world_conn!(world);
            let instrument_id = instrument_id_by_symbol(&conn, &symbol);
            let account_id = world.account_id(&account_name);
            let currency_code: String = conn
                .query_row(
                    "SELECT currency_code FROM accounts WHERE id=?1",
                    params![account_id],
                    |r| r.get(0),
                )
                .unwrap_or_else(|_| panic!("账户不存在: {account_name}"));
            (instrument_id, account_id, currency_code)
        };
        inputs.push(TransactionInput {
            currency_code,
            fee_cents: Some(get(row, "手续费").parse().expect("手续费必须是整数")),
            ..trade_input(
                kind,
                &instrument_id,
                get(row, "数量").parse().expect("数量必须是数字"),
                get(row, "单价").parse().expect("单价必须是整数"),
                &account_id,
                &get(row, "日期"),
            )
        });
    }
    let count = inputs.len();

    let results = world_write!(world, |conn| TransactionBatch::run(conn, inputs, true))
        .expect("批量导入投资交易失败")
        .results;
    assert_eq!(results.len(), count, "导入结果行数应与提交行数一致");

    world.txn.last_batch_results = results;
}

#[then(expr = "导入的投资交易应有 {int} 行全部成功")]
fn assert_imported_trades_all_success(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(
        world.txn.last_batch_results.len(),
        expected,
        "导入结果行数不符"
    );
    for r in &world.txn.last_batch_results {
        assert!(r.success, "导入行应成功: {r:?}");
        assert!(!r.duplicate, "首次导入不应命中重复: {r:?}");
        assert!(r.id.is_some(), "成功行应返回交易 id: {r:?}");
    }
}

#[then(expr = "标的 {string} 持仓应为 {float}")]
fn assert_holding(world: &mut LedgerWorld, symbol: String, quantity: f64) {
    let qty: f64 = {
        let conn = world_conn!(world);
        conn.query_row(
            "SELECT COALESCE(SUM(l.remaining_quantity), 0.0) \
             FROM security_lots l \
             JOIN instruments i ON i.id = l.instrument_id \
             JOIN accounts a ON a.id = l.account_id \
             WHERE i.symbol=?1 AND a.is_deleted=0 AND l.remaining_quantity > 0",
            params![symbol],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert!(
        (qty - quantity).abs() < 1e-9,
        "标的 {symbol} 持仓数量不符：期望 {quantity}，实际 {qty}"
    );
}
