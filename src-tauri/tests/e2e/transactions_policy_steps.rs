//! 交易流水直挂保单 BDD 步骤（issue #361 / spec #358 / ADR-0051 决策 3）。
//!
//! 经 `commands::transactions` 的 `*_internal` seam 断言外部可观察行为：
//! 手动挂单归属、改挂/清除、不可挂单 kind 被行为层拒绝、挂单引用不存在的保单被拒、
//! 软删保单历史引用保留不置空且保持原挂单可继续编辑（与商户「保持历史引用」同款语义）。
//! 商户/保单/标的 Given 复用 `merchants_steps.rs` / `policies_steps.rs` /
//! `instruments_steps.rs` 已注册步骤。

use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::error::AppError;
use tauri_app_lib::models::TransactionInput;
use tauri_app_lib::transaction::TransactionBatch;
use tauri_app_lib::transaction::amount::TransactionKind;
use tauri_app_lib::transaction::{create_transaction_internal, update_transaction_internal};

use crate::common::query_all_transactions;
use crate::world::LedgerWorld;

/// 按保单号查保单 id（场景内保单号唯一；不存在返回 None 供 404 路径直提裸值）。
fn policy_id_by_number(world: &LedgerWorld, number: &str) -> Option<String> {
    world_conn!(world)
        .query_row(
            "SELECT id FROM policies WHERE policy_number=?1 AND is_deleted=0",
            params![number],
            |r| r.get::<_, String>(0),
        )
        .ok()
}

/// 组装带可选保单引用的交易入参（挂单场景统一入口）。
fn input_with_policy(
    world: &LedgerWorld,
    kind: TransactionKind,
    amount: i64,
    account_name: &str,
    date: &str,
    policy_id: Option<String>,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        kind,
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: world.account_id(account_name),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        policy_id,
        refund_of_transaction_id: None,
        note: None,
        date: date.into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    }
}

// ---------------------------------------------------------------------------
// When：创建（挂单 / 拒绝路径）
// ---------------------------------------------------------------------------

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
        TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
        amount,
        &account_name,
        &date,
        Some(policy_id),
    );
    let result = world
        .db
        .write(|conn| create_transaction_internal(conn, input));
    assert!(
        result.is_ok(),
        "创建挂单交易失败: {:?}",
        result.err().map(|e| e.to_string())
    );
    world.last_transaction_id = Some(result.unwrap().id);
    world.transactions_list = query_all_transactions(&world_conn!(world));
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
        TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
        amount,
        &account_name,
        &date,
        Some(policy_id),
    );
    let result = create_transaction_internal(&world_conn!(world), input);
    world.last_error = match result {
        Err(AppError::Coded { message, .. }) => Some(message),
        Ok(_) => Some("预期失败但成功了".into()),
        Err(e) => Some(e.to_string()),
    };
}

/// 修改路径同样收口在行为层：转账/买入等不可挂单 kind 携带保单，plan 阶段拒绝。
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
    let result = create_transaction_internal(&world_conn!(world), input);
    world.last_error = match result {
        Err(AppError::Coded { message, .. }) => Some(message),
        Ok(_) => Some("预期失败但成功了".into()),
        Err(e) => Some(e.to_string()),
    };
}

/// buy 携带保单：行为层 plan 在投资域 prepare 之前即拒绝（准入收口先于副作用）。
#[when(expr = "尝试买入标的 {string} 数量 {int} 单价 {int} 到投资账户 {string} 挂保单 {string}")]
fn try_buy_with_policy(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price: i64,
    account_name: String,
    policy_id: String,
) {
    let instrument_id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .expect("买入挂单步骤：标的应已存在");
    let input = TransactionInput {
        merchant_name: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: world.account_id(&account_name),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        policy_id: Some(policy_id),
        refund_of_transaction_id: None,
        note: None,
        date: "2026-05-01".into(),
        instrument_id: Some(instrument_id),
        quantity: Some(quantity as f64),
        price_cents: Some(price),
        fee_cents: None,
        idempotency_key: None,
    };
    let result = create_transaction_internal(&world_conn!(world), input);
    world.last_error = match result {
        Err(AppError::Coded { message, .. }) => Some(message),
        Ok(_) => Some("预期失败但成功了".into()),
        Err(e) => Some(e.to_string()),
    };
}

/// sell 携带保单：买入铺垫后尝试卖出挂单，plan 阶段拒绝（不应产生卖出副作用）。
#[when(expr = "尝试卖出标的 {string} 数量 {int} 单价 {int} 从投资账户 {string} 挂保单 {string}")]
fn try_sell_with_policy(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price: i64,
    account_name: String,
    policy_id: String,
) {
    let instrument_id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .expect("卖出挂单步骤：标的应已存在");
    let input = TransactionInput {
        merchant_name: None,
        kind: TransactionKind::Sell,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: world.account_id(&account_name),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        policy_id: Some(policy_id),
        refund_of_transaction_id: None,
        note: None,
        date: "2026-05-02".into(),
        instrument_id: Some(instrument_id),
        quantity: Some(quantity as f64),
        price_cents: Some(price),
        fee_cents: None,
        idempotency_key: None,
    };
    let result = create_transaction_internal(&world_conn!(world), input);
    world.last_error = match result {
        Err(AppError::Coded { message, .. }) => Some(message),
        Ok(_) => Some("预期失败但成功了".into()),
        Err(e) => Some(e.to_string()),
    };
}

/// refund 携带保单：现金流入记 income 挂单而非 refund（ADR-0051 决策 4），
/// refund 不在准入集——携带保单在 plan 阶段被拒。
#[when(expr = "尝试创建退款 金额 {int} 关联最近支出 日期 {string} 挂保单 {string}")]
fn try_refund_with_policy(world: &mut LedgerWorld, amount: i64, date: String, policy_id: String) {
    let source_id = world
        .last_transaction_id
        .clone()
        .expect("退款挂单步骤：应有原支出交易");
    let input = TransactionInput {
        merchant_name: None,
        kind: TransactionKind::Refund,
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: world.account_id("A账户"),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        policy_id: Some(policy_id),
        refund_of_transaction_id: Some(source_id),
        note: None,
        date,
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let result = create_transaction_internal(&world_conn!(world), input);
    world.last_error = match result {
        Err(AppError::Coded { message, .. }) => Some(message),
        Ok(_) => Some("预期失败但成功了".into()),
        Err(e) => Some(e.to_string()),
    };
}

// ---------------------------------------------------------------------------
// When：批量导入写路径（AC4：行为层准入对所有写路径一致生效）
// ---------------------------------------------------------------------------

/// 批量导入挂单交易：与 HTTP 批量导入端点同走 `TransactionBatch::run`，
/// 每行经 behavior::create 自然受 kind 准入约束。表格列：kind | 金额 | 币种 | 账户 | 日期 | 保单号。
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
            merchant_name: None,
            kind: TransactionKind::parse(&get(row, "kind"))
                .unwrap_or_else(|e| panic!("非法 kind: {}（{e}）", get(row, "kind"))),
            amount_cents: get(row, "金额").parse().expect("金额必须是整数"),
            currency_code: get(row, "币种"),
            account_id: world.account_id(&get(row, "账户")),
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            policy_id: policy_id_by_number(world, &get(row, "保单号")),
            refund_of_transaction_id: None,
            note: None,
            date: get(row, "日期"),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        })
        .collect();
    let _ = world
        .db
        .write(|conn| TransactionBatch::run(conn, inputs, true))
        .expect("批量导入挂单交易失败");
    world.transactions_list = query_all_transactions(&world_conn!(world));
}

// ---------------------------------------------------------------------------
// When：修改（改挂 / 清除 / 保持原挂单）
// ---------------------------------------------------------------------------

#[when(expr = "修改最近交易挂保单 {string}")]
fn update_last_txn_policy(world: &mut LedgerWorld, policy_number: String) {
    let id = world.last_transaction_id.clone().expect("没有可修改的交易");
    let policy_id = policy_id_by_number(world, &policy_number)
        .unwrap_or_else(|| panic!("改挂步骤：保单 {policy_number} 应已存在"));
    let existing = world
        .transactions_list
        .iter()
        .find(|t| t.id == id)
        .expect("原交易不存在")
        .clone();
    let input = TransactionInput {
        policy_id: Some(policy_id),
        ..existing_to_input(&existing)
    };
    update_and_refresh(world, &id, input);
}

#[when(expr = "修改最近交易清除挂单")]
fn clear_last_txn_policy(world: &mut LedgerWorld) {
    let id = world.last_transaction_id.clone().expect("没有可修改的交易");
    let existing = world
        .transactions_list
        .iter()
        .find(|t| t.id == id)
        .expect("原交易不存在")
        .clone();
    let input = TransactionInput {
        policy_id: None,
        ..existing_to_input(&existing)
    };
    update_and_refresh(world, &id, input);
}

/// 保持原挂单修改其他字段：提交 policy_id 与原值相同 → 行为层「保持历史引用」
/// 跳过在用校验，已软删保单的历史交易仍可修改其他字段（issue #188 / ADR-0028 语义）。
#[when(expr = "修改第 {int} 条交易备注 {string} 保持原挂单")]
fn update_keep_policy(world: &mut LedgerWorld, index: usize, note: String) {
    let existing = world
        .transactions_list
        .get(index - 1)
        .unwrap_or_else(|| panic!("交易列表第 {index} 条不存在"))
        .clone();
    let id = existing.id.clone();
    world.last_transaction_id = Some(id.clone());
    let input = TransactionInput {
        note: Some(note),
        ..existing_to_input(&existing)
    };
    update_and_refresh(world, &id, input);
}

/// 既有交易 → 全量替换入参（修改是全字段替换，未提及字段原样保留）。
fn existing_to_input(existing: &tauri_app_lib::models::Transaction) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        kind: existing.kind,
        amount_cents: existing.amount_cents,
        currency_code: existing.currency_code.clone(),
        account_id: existing.account_id.clone(),
        to_account_id: existing.to_account_id.clone(),
        category_id: existing.category_id.clone(),
        merchant_id: existing.merchant_id.clone(),
        policy_id: existing.policy_id.clone(),
        refund_of_transaction_id: existing.refund_of_transaction_id.clone(),
        note: existing.note.clone(),
        date: existing.date.clone(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    }
}

fn update_and_refresh(world: &mut LedgerWorld, id: &str, input: TransactionInput) {
    let result = update_transaction_internal(&world_conn!(world), id, input);
    assert!(
        result.is_ok(),
        "修改交易失败: {:?}",
        result.err().map(|e| e.to_string())
    );
    world.transactions_list = query_all_transactions(&world_conn!(world));
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "第 {int} 条交易挂单应为保单号 {string}")]
fn check_txn_policy(world: &mut LedgerWorld, index: usize, policy_number: String) {
    let txn = world
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
        .transactions_list
        .get(index - 1)
        .unwrap_or_else(|| panic!("交易列表第 {index} 条不存在"));
    assert!(
        txn.policy_id.is_none(),
        "第 {index} 条交易应无挂单，实际: {:?}",
        txn.policy_id
    );
}

/// 历史引用保留不置空（ADR-0051 决策 5）：直接查库内行（含软删保单的 id），
/// 引用值非空即通过——保单是否软删不影响引用保留。
#[then(expr = "第 {int} 条交易挂单引用应保留（软删保单不置空）")]
fn check_txn_policy_kept(world: &mut LedgerWorld, index: usize) {
    let txn = world
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
