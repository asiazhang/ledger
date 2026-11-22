use cucumber::{then, when};
use rusqlite::params;

use ledger_infra::error::AppError;
use ledger_transaction::TransactionBatch;
use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;

use crate::common::{instrument_id_by_symbol, query_all_transactions};
use crate::step_inputs::existing_input;
use crate::step_inputs::{buy_input, parse_kind, plain_input, refund_input, sell_input};
use crate::step_verbs::{
    create_transaction_verb, try_create_transaction_verb, update_transaction_verb,
};
use crate::world::LedgerWorld;

fn policy_id_by_number(world: &LedgerWorld, number: &str) -> Option<String> {
    world_conn!(world)
        .query_row(
            "SELECT id FROM policies WHERE policy_number=?1 AND is_deleted=0",
            params![number],
            |r| r.get::<_, String>(0),
        )
        .ok()
}

fn input_with_policy(
    world: &LedgerWorld,
    kind: TransactionKind,
    amount: i64,
    account_name: &str,
    date: &str,
    policy_id: Option<String>,
) -> TransactionInput {
    TransactionInput {
        policy_id,
        ..plain_input(kind, amount, &world.account_id(account_name), date)
    }
}

fn last_error_of(result: Result<String, AppError>) -> Option<String> {
    match result {
        Err(AppError::Coded { message, .. }) => Some(message),
        Ok(_) => Some("预期失败但成功了".into()),
        Err(e) => Some(e.to_string()),
    }
}

#[when(expr = "创建交易 类型 {string} 金额 {int} 到账户 {string} 日期 {string} 挂保单 {string}")]
fn create_txn_with_policy(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    account_name: String,
    date: String,
    policy_number: String,
) {
    let policy_id = policy_id_by_number(world, &policy_number)
        .unwrap_or_else(|| panic!("挂单步骤：保单 {policy_number} 应已存在"));
    let input = input_with_policy(
        world,
        parse_kind(&kind),
        amount,
        &account_name,
        &date,
        Some(policy_id),
    );
    create_transaction_verb(world, input);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(
    expr = "尝试创建交易 类型 {string} 金额 {int} 到账户 {string} 日期 {string} 挂保单 {string}"
)]
fn try_create_txn_with_policy(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    account_name: String,
    date: String,
    policy_id: String,
) {
    let input = input_with_policy(
        world,
        parse_kind(&kind),
        amount,
        &account_name,
        &date,
        Some(policy_id),
    );
    world.last_error = last_error_of(try_create_transaction_verb(world, input));
}

#[when(
    expr = "尝试创建转账 金额 {int} 从账户 {string} 到账户 {string} 日期 {string} 挂保单 {string}"
)]
fn try_transfer_with_policy(
    world: &mut LedgerWorld,
    amount: i64,
    from_account: String,
    to_account: String,
    date: String,
    policy_id: String,
) {
    let mut input = input_with_policy(
        world,
        TransactionKind::Transfer,
        amount,
        &from_account,
        &date,
        Some(policy_id),
    );
    input.to_account_id = Some(world.account_id(&to_account));
    world.last_error = last_error_of(try_create_transaction_verb(world, input));
}

#[when(expr = "尝试买入标的 {string} 数量 {int} 单价 {int} 到投资账户 {string} 挂保单 {string}")]
fn try_buy_with_policy(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price: i64,
    account_name: String,
    policy_id: String,
) {
    let instrument_id = instrument_id_by_symbol(&world_conn!(world), &symbol);
    let account_id = world.account_id(&account_name);
    let input = TransactionInput {
        policy_id: Some(policy_id),
        ..buy_input(
            &instrument_id,
            quantity as f64,
            Some(price),
            &account_id,
            "2026-05-01",
        )
    };
    world.last_error = last_error_of(try_create_transaction_verb(world, input));
}

#[when(expr = "尝试卖出标的 {string} 数量 {int} 单价 {int} 从投资账户 {string} 挂保单 {string}")]
fn try_sell_with_policy(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price: i64,
    account_name: String,
    policy_id: String,
) {
    let instrument_id = instrument_id_by_symbol(&world_conn!(world), &symbol);
    let account_id = world.account_id(&account_name);
    let input = TransactionInput {
        policy_id: Some(policy_id),
        ..sell_input(
            &instrument_id,
            quantity as f64,
            Some(price),
            &account_id,
            "2026-05-02",
        )
    };
    world.last_error = last_error_of(try_create_transaction_verb(world, input));
}

#[when(expr = "尝试创建退款 金额 {int} 关联最近支出 日期 {string} 挂保单 {string}")]
fn try_refund_with_policy(world: &mut LedgerWorld, amount: i64, date: String, policy_id: String) {
    let source_id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("退款挂单步骤：应有原支出交易");
    let account_id = world.account_id("A账户");
    let input = TransactionInput {
        policy_id: Some(policy_id),
        ..refund_input(amount, &account_id, &source_id, &date)
    };
    world.last_error = last_error_of(try_create_transaction_verb(world, input));
}

#[when(expr = "批量导入挂单交易")]
fn batch_import_with_policy(world: &mut LedgerWorld, step: &cucumber::gherkin::Step) {
    let table = step.table.as_ref().expect("批量导入挂单步骤缺少数据表");
    let headers = &table.rows[0];
    let col = |name: &str| headers.iter().position(|h| h == name);
    let get = |row: &[String], name: &str| {
        col(name)
            .and_then(|i| row.get(i).cloned())
            .unwrap_or_default()
    };
    let inputs: Vec<TransactionInput> = table
        .rows
        .iter()
        .skip(1)
        .map(|row| TransactionInput {
            currency_code: get(row, "币种"),
            policy_id: policy_id_by_number(world, &get(row, "保单号")),
            ..plain_input(
                parse_kind(&get(row, "kind")),
                get(row, "金额").parse().expect("金额必须是整数"),
                &world.account_id(&get(row, "账户")),
                &get(row, "日期"),
            )
        })
        .collect();
    let _ = world_write!(world, |conn| TransactionBatch::run(conn, inputs, true))
        .expect("批量导入挂单交易失败");
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "修改最近交易挂保单 {string}")]
fn update_last_txn_policy(world: &mut LedgerWorld, policy_number: String) {
    let id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("没有可修改的交易");
    let policy_id = policy_id_by_number(world, &policy_number)
        .unwrap_or_else(|| panic!("改挂步骤：保单 {policy_number} 应已存在"));
    let existing = world
        .txn
        .transactions_list
        .iter()
        .find(|t| t.id == id)
        .expect("原交易不存在");
    let input = TransactionInput {
        policy_id: Some(policy_id),
        ..existing_input(existing)
    };
    update_and_refresh(world, &id, input);
}

#[when(expr = "修改最近交易清除挂单")]
fn clear_last_txn_policy(world: &mut LedgerWorld) {
    let id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("没有可修改的交易");
    let existing = world
        .txn
        .transactions_list
        .iter()
        .find(|t| t.id == id)
        .expect("原交易不存在");
    let input = TransactionInput {
        policy_id: None,
        ..existing_input(existing)
    };
    update_and_refresh(world, &id, input);
}

#[when(expr = "修改第 {int} 条交易备注 {string} 保持原挂单")]
fn update_keep_policy(world: &mut LedgerWorld, index: usize, note: String) {
    let existing = world
        .txn
        .transactions_list
        .get(index - 1)
        .unwrap_or_else(|| panic!("交易列表第 {index} 条不存在"));
    let id = existing.id.clone();
    world.txn.last_transaction_id = Some(id.clone());
    let input = TransactionInput {
        note: Some(note),
        ..existing_input(existing)
    };
    update_and_refresh(world, &id, input);
}

fn update_and_refresh(world: &mut LedgerWorld, id: &str, input: TransactionInput) {
    update_transaction_verb(world, id, input);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[then(expr = "第 {int} 条交易挂单应为保单号 {string}")]
fn check_txn_policy(world: &mut LedgerWorld, index: usize, policy_number: String) {
    let txn = world
        .txn
        .transactions_list
        .get(index - 1)
        .unwrap_or_else(|| panic!("交易列表第 {index} 条不存在"));
    let policy_id = txn
        .policy_id
        .as_ref()
        .unwrap_or_else(|| panic!("第 {index} 条交易应挂保单，实际无挂单"));
    let number: String = world_conn!(world)
        .query_row(
            "SELECT policy_number FROM policies WHERE id=?1",
            params![policy_id],
            |r| r.get(0),
        )
        .expect("挂单引用的保单应存在");
    assert_eq!(number, policy_number, "挂单保单号不匹配");
}

#[then(expr = "第 {int} 条交易应无挂单")]
fn check_txn_no_policy(world: &mut LedgerWorld, index: usize) {
    let txn = world
        .txn
        .transactions_list
        .get(index - 1)
        .unwrap_or_else(|| panic!("交易列表第 {index} 条不存在"));
    assert!(
        txn.policy_id.is_none(),
        "第 {index} 条交易应无挂单，实际: {:?}",
        txn.policy_id
    );
}

#[then(expr = "第 {int} 条交易挂单引用应保留（软删保单不置空）")]
fn check_txn_policy_kept(world: &mut LedgerWorld, index: usize) {
    let txn = world
        .txn
        .transactions_list
        .get(index - 1)
        .unwrap_or_else(|| panic!("交易列表第 {index} 条不存在"));
    let kept: Option<String> = world_conn!(world)
        .query_row(
            "SELECT policy_id FROM transactions WHERE id=?1",
            params![txn.id],
            |r| r.get(0),
        )
        .expect("交易行应存在");
    assert!(
        kept.is_some(),
        "软删保单的历史流水引用应保留不置空，实际: {kept:?}"
    );
}
