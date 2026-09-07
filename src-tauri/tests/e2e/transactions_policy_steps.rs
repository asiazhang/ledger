//! 交易流水直挂保单 BDD 步骤（issue #361 / spec #358 / ADR-0051 决策 3；issue #761
//! 迁移：交易输入构造收编 L1 步骤输入工厂——中性底座/买卖/退款工厂 + 保单与币种
//! 冷字段覆盖，写入收编 L2 动词，错误断言路径改走 try 形态，断言语义不变）。
//!
//! 经行为层 `*_internal` seam 断言外部可观察行为：手动挂单归属、改挂/清除、
//! 不可挂单 kind 被行为层拒绝、挂单引用不存在的保单被拒、软删保单历史引用保留
//! 不置空且保持原挂单可继续编辑（与商户「保持历史引用」同款语义）。
//! 商户/保单/标的 Given 复用 `merchants_steps.rs` / `policies_steps.rs` /
//! `instruments_steps.rs` 已注册步骤；批量导入直走 `TransactionBatch::run`
//! （与 HTTP 批量导入端点同一写接缝）。

use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::error::AppError;
use tauri_app_lib::transaction::TransactionBatch;
use tauri_app_lib::transaction::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::common::{instrument_id_by_symbol, query_all_transactions};
use crate::step_inputs::existing_input;
use crate::step_inputs::{buy_input, parse_kind, plain_input, refund_input, sell_input};
use crate::step_verbs::{
    create_transaction_verb, try_create_transaction_verb, update_transaction_verb,
};
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

/// 组装带可选保单引用的交易入参（挂单场景统一入口）：中性底座 + 保单覆盖。
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

/// 记录「预期失败但成功了」/ 行为层错误到 `world.last_error`（挂单拒绝路径共用）。
fn last_error_of(result: Result<String, AppError>) -> Option<String> {
    match result {
        Err(AppError::Coded { message, .. }) => Some(message),
        Ok(_) => Some("预期失败但成功了".into()),
        Err(e) => Some(e.to_string()),
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
    world.last_error = last_error_of(try_create_transaction_verb(world, input));
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

/// refund 携带保单：现金流入记 income 挂单而非 refund（ADR-0051 决策 4），
/// refund 不在准入集——携带保单在 plan 阶段被拒。
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
    let _ = world
        .db
        .write(|conn| TransactionBatch::run(conn, inputs, true))
        .expect("批量导入挂单交易失败");
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

// ---------------------------------------------------------------------------
// When：修改（改挂 / 清除 / 保持原挂单）
// ---------------------------------------------------------------------------

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

/// 保持原挂单修改其他字段：提交 policy_id 与原值相同 → 行为层「保持历史引用」
/// 跳过在用校验，已软删保单的历史交易仍可修改其他字段（issue #188 / ADR-0028 语义）。
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

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

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

/// 历史引用保留不置空（ADR-0051 决策 5）：直接查库内行（含软删保单的 id），
/// 引用值非空即通过——保单是否软删不影响引用保留。
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
