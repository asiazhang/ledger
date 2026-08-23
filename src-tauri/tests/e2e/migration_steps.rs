use cucumber::gherkin::Step;
use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::commands::accounts::{
    create_account_idempotent_internal, list_account_balances_for_api_internal,
};
use tauri_app_lib::commands::transactions::{
    create_transactions_internal, delete_transaction_internal, list_transactions_internal,
};
use tauri_app_lib::models::{AccountInput, AccountType, TransactionInput, TransactionListFilter};

use crate::common::query_all_transactions;
use crate::world::{ImportedRow, LedgerWorld};

/// 批量导入：模拟 AI 迁移，走与 HTTP 批量导入一致的 `create_transactions_internal`（dedup=true）。
/// 表格列：kind | 金额 | 币种 | 账户 | 转入账户 | 日期 [| 备注]
#[when(expr = "批量导入交易")]
fn batch_import(world: &mut LedgerWorld, #[step] step: &Step) {
    let table = step.table.as_ref().expect("批量导入步骤缺少数据表");
    let rows: Vec<ImportedRow> = table
        .rows
        .iter()
        .skip(1)
        .map(|row| ImportedRow {
            kind: row[0].clone(),
            amount_cents: row[1].parse().expect("金额必须是整数"),
            currency_code: row[2].clone(),
            account_name: row[3].clone(),
            to_account_name: (!row[4].is_empty()).then(|| row[4].clone()),
            note: row.get(6).cloned().filter(|s| !s.is_empty()),
            date: row[5].clone(),
        })
        .collect();
    let inputs: Vec<TransactionInput> = rows.iter().map(|r| r.to_input(world)).collect();
    let results = create_transactions_internal(&world.conn, inputs, true).expect("批量导入失败");
    world.last_import_rows = rows;
    world.last_batch_results = results;
    world.transactions_list = query_all_transactions(&world.conn);
}

/// 重跑刚才的批量导入：与首次导入相同的行、相同的 dedup 语义。
#[when(expr = "重跑刚才的批量导入")]
fn reimport(world: &mut LedgerWorld) {
    let inputs: Vec<TransactionInput> = world
        .last_import_rows
        .iter()
        .map(|r| r.to_input(world))
        .collect();
    let results =
        create_transactions_internal(&world.conn, inputs, true).expect("重跑批量导入失败");
    world.last_batch_results = results;
    world.transactions_list = query_all_transactions(&world.conn);
}

/// 删除备注为指定值的交易（软删除，与 HTTP DELETE 端点共用 `delete_transaction_internal`）。
#[when(expr = "删除备注为 {string} 的交易")]
fn delete_txn_by_note(world: &mut LedgerWorld, note: String) {
    let id: String = world
        .conn
        .query_row(
            "SELECT id FROM transactions WHERE note=?1 AND is_deleted=0 ORDER BY created_at DESC LIMIT 1",
            params![note],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| panic!("未找到备注为 '{note}' 的交易"));
    delete_transaction_internal(&world.conn, &id).expect("删除交易失败");
    world.transactions_list = query_all_transactions(&world.conn);
}

/// 查询全部未删除账户的实时余额（含黑洞账户），快照到 world.balances。
#[when(expr = "查询全部账户余额")]
fn query_balances(world: &mut LedgerWorld) {
    let balances = list_account_balances_for_api_internal(&world.conn).expect("查询账户余额失败");
    world.balances = balances
        .into_iter()
        .map(|ab| (ab.account.name, (ab.balance_cents, ab.account.is_hidden)))
        .collect();
}

/// 重跑导入创建账户：幂等创建（与 HTTP POST /api/v1/accounts 语义一致），
/// 软删除后重导可重新建回。
#[when(expr = "重跑导入创建账户 {string} 类型 {string} 币种 {string}")]
fn reimport_create_account(world: &mut LedgerWorld, name: String, kind: String, currency: String) {
    let account_kind: AccountType = kind
        .parse()
        .unwrap_or_else(|_| panic!("未知账户类型: {kind}"));
    let id = create_account_idempotent_internal(
        &world.conn,
        AccountInput {
            name: name.clone(),
            kind: account_kind,
            currency_code: currency,
            initial_balance_cents: None,
        },
    )
    .expect("幂等创建账户失败");
    world.account_name_to_id.insert(name, id);
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "读回交易 应包含 {int} 条记录")]
fn readback_count(world: &mut LedgerWorld, expected: i64) {
    let result = list_transactions_internal(&world.conn, &TransactionListFilter::default())
        .expect("读回交易失败");
    assert_eq!(result.items.len() as i64, expected, "读回交易数量不匹配");
    world.transactions_list = result.items;
}

#[then(expr = "读回 {string} 至 {string} 交易 应包含 {int} 条记录")]
fn readback_range(world: &mut LedgerWorld, from: String, to: String, expected: i64) {
    let result = list_transactions_internal(
        &world.conn,
        &TransactionListFilter {
            from: Some(from.clone()),
            to: Some(to.clone()),
            ..Default::default()
        },
    )
    .expect("读回交易失败");
    assert_eq!(
        result.items.len() as i64,
        expected,
        "日期区间 [{from}, {to}] 交易数量不匹配"
    );
}

#[then(expr = "读回 账户 {string} 的交易 应包含 {int} 条记录")]
fn readback_account(world: &mut LedgerWorld, name: String, expected: i64) {
    let account_id = world.account_id(&name);
    let result = list_transactions_internal(
        &world.conn,
        &TransactionListFilter {
            account_id: Some(account_id),
            ..Default::default()
        },
    )
    .expect("读回交易失败");
    assert_eq!(
        result.items.len() as i64,
        expected,
        "账户 '{name}' 的交易数量不匹配"
    );
}

#[then(expr = "读回 kind 为 {string} 的交易 应包含 {int} 条记录 金额合计 {int}")]
fn readback_kind_amount(
    world: &mut LedgerWorld,
    kind: String,
    expected_count: i64,
    expected_sum: i64,
) {
    let result = list_transactions_internal(
        &world.conn,
        &TransactionListFilter {
            kind: Some(kind.clone()),
            ..Default::default()
        },
    )
    .expect("读回交易失败");
    assert_eq!(
        result.items.len() as i64,
        expected_count,
        "kind={kind} 交易数量不匹配"
    );
    let sum: i64 = result.items.iter().map(|t| t.amount_cents).sum();
    assert_eq!(sum, expected_sum, "kind={kind} 金额合计不匹配");
}

#[then(expr = "读回交易 应包含 金额 {int} 的记录")]
fn readback_with_amount(world: &mut LedgerWorld, amount: i64) {
    world.transactions_list = query_all_transactions(&world.conn);
    assert!(
        world
            .transactions_list
            .iter()
            .any(|t| t.amount_cents == amount),
        "读回列表应包含金额 {amount} 的记录"
    );
}

#[then(expr = "读回交易 应不包含 金额 {int} 的记录")]
fn readback_without_amount(world: &mut LedgerWorld, amount: i64) {
    world.transactions_list = query_all_transactions(&world.conn);
    assert!(
        !world
            .transactions_list
            .iter()
            .any(|t| t.amount_cents == amount),
        "读回列表不应包含金额 {amount} 的记录"
    );
}

#[then(expr = "余额清单应包含 {int} 个账户")]
fn balance_count(world: &mut LedgerWorld, expected: i64) {
    assert_eq!(
        world.balances.len() as i64,
        expected,
        "余额清单账户数量不匹配"
    );
}

#[then(expr = "账户 {string} 余额应为 {int}")]
fn balance_of_name(world: &mut LedgerWorld, name: String, expected: i64) {
    let (actual, _) = world.balances.get(&name).unwrap_or_else(|| {
        panic!(
            "余额清单应包含账户 '{}'，实际为 {:?}",
            name,
            world.balances.keys().collect::<Vec<_>>()
        )
    });
    assert_eq!(*actual, expected, "账户 '{name}' 余额不匹配");
}

#[then(expr = "账户 {string} 应为黑洞账户")]
fn check_is_hidden(world: &mut LedgerWorld, name: String) {
    let (_, is_hidden) = world.balances.get(&name).unwrap_or_else(|| {
        panic!(
            "余额清单应包含账户 '{}'，实际为 {:?}",
            name,
            world.balances.keys().collect::<Vec<_>>()
        )
    });
    assert!(*is_hidden, "账户 '{name}' 应为黑洞账户（is_hidden=true）");
}

#[then(expr = "账户 {string} 不应为黑洞账户")]
fn check_not_hidden(world: &mut LedgerWorld, name: String) {
    let (_, is_hidden) = world.balances.get(&name).unwrap_or_else(|| {
        panic!(
            "余额清单应包含账户 '{}'，实际为 {:?}",
            name,
            world.balances.keys().collect::<Vec<_>>()
        )
    });
    assert!(
        !*is_hidden,
        "账户 '{name}' 不应为黑洞账户（is_hidden=false）"
    );
}

#[then(expr = "最近一次导入应有 {int} 条去重跳过 {int} 条新写入")]
fn check_batch_results(world: &mut LedgerWorld, duplicates: i64, new: i64) {
    let dup_count = world
        .last_batch_results
        .iter()
        .filter(|r| r.duplicate)
        .count();
    let new_count = world
        .last_batch_results
        .iter()
        .filter(|r| !r.duplicate && r.success)
        .count();
    assert_eq!(dup_count as i64, duplicates, "去重跳过条数不匹配");
    assert_eq!(new_count as i64, new, "新写入条数不匹配");
}
