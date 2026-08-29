//! 商户消费排行 e2e 步骤定义（issue #192）。
//!
//! 排行口径由核心函数 `merchant_shares_rows`（命令层同款注入）查询：
//! `expense_net`（毛支出 − 退款）按商户聚合、本位币口径。交易夹具走与真实
//! 写路径一致的行为层（`insert_transaction`），复用商户/交易步骤模块的既有步骤。

use cucumber::{then, when};

use tauri_app_lib::commands::reports::merchant_shares_rows;
use tauri_app_lib::commands::transactions::insert_transaction;
use tauri_app_lib::models::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::common::query_all_transactions;
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

/// 创建带商户的跨币种交易（本位币折算由写路径 `convert_to_native` 完成）。
#[when(
    expr = "创建交易 类型 {string} 金额 {int} 币种 {string} 到账户 {string} 日期 {string} 商户 {string}"
)]
fn create_txn_with_merchant_currency(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    currency: String,
    account_name: String,
    date: String,
    merchant_name: String,
) {
    let input = TransactionInput {
        merchant_name: None,
        kind: TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
        amount_cents: amount,
        currency_code: currency,
        account_id: world.account_id(&account_name),
        to_account_id: None,
        category_id: None,
        merchant_id: Some(world.merchant_id(&merchant_name)),
        refund_of_transaction_id: None,
        note: None,
        date,
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let result = insert_transaction(
        &world.db.conn.lock().unwrap_or_else(|e| e.into_inner()),
        input,
    );
    assert!(result.is_ok(), "创建交易失败: {:?}", result.err());
    world.last_transaction_id = Some(result.unwrap());
    world.transactions_list =
        query_all_transactions(&world.db.conn.lock().unwrap_or_else(|e| e.into_inner()));
}

/// 查询指定年份的商户消费排行（命令层同款核心函数注入）。
#[when(expr = "查询 {int} 年商户排行")]
fn query_merchant_shares(world: &mut LedgerWorld, year: i64) {
    world.last_merchant_shares = merchant_shares_rows(
        &world.db.conn.lock().unwrap_or_else(|e| e.into_inner()),
        year,
    )
    .expect("查询商户排行失败");
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

/// 排行行数断言。
#[then(expr = "商户排行应为 {int} 行")]
fn check_merchant_ranking_len(world: &mut LedgerWorld, n: usize) {
    assert_eq!(
        world.last_merchant_shares.len(),
        n,
        "排行行数不符：实际 {:?}",
        world
            .last_merchant_shares
            .iter()
            .map(|s| (s.merchant_name.as_str(), s.amount_cents))
            .collect::<Vec<_>>()
    );
}

/// 排行第 {index} 名断言：商户名（现名，改名即时生效）+ 本位币净支出，
/// 顺序即排行顺序（净额降序）。
#[then(expr = "商户排行第 {int} 名应为 {string} 金额 {int}")]
fn check_merchant_ranking_row(world: &mut LedgerWorld, index: usize, name: String, amount: i64) {
    let share = world
        .last_merchant_shares
        .get(index - 1)
        .unwrap_or_else(|| panic!("商户排行第 {index} 名不存在"));
    assert_eq!(share.merchant_name, name, "排行第 {index} 名商户不符");
    assert_eq!(share.amount_cents, amount, "商户 '{name}' 净支出不符");
}

/// 商户契约回归「名字字典」（issue #223）：排行响应序列化后不应再含指定字段
/// （icon/color 已退役；排行行只含名称与金额）。
#[then(expr = "商户排行响应 JSON 不含字段 {string}")]
fn check_merchant_shares_json_not_contain_field(world: &mut LedgerWorld, field: String) {
    assert!(
        !world.last_merchant_shares.is_empty(),
        "商户排行为空，无法校验响应字段契约"
    );
    for s in &world.last_merchant_shares {
        let json = serde_json::to_value(s).expect("商户排行行序列化失败");
        assert!(
            json.get(&field).is_none(),
            "商户排行响应不应含字段 '{field}'，实际: {json}"
        );
    }
}
