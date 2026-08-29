//! 物品（Item）BDD 步骤 · 创建主题（issue #115 / spec #113）：创建与读回。
//!
//! 经 `commands::item` 的 `*_internal` seam 断言外部可观察行为：
//! 创建读回、每天成本（`item::cost` 口径，含起止两端的日历天数）、金额折算、
//! 写后发失效信号（notify 注入，生产路径发 `ledger:changed`）。
//! 通用列表/字段断言（物品列表应包含 N 件、第 N 件名称/金额/状态/已用天数、
//! 唯一 ID 与审计字段、失效信号）亦收敛于此，供其余主题模块的 feature 场景
//! 复用（cucumber 步骤全局注册，各模块步骤均可被任一 feature 命中）。

use cucumber::{then, when};

use tauri_app_lib::commands::item::{create_item_internal, list_items_internal};
use tauri_app_lib::commands::transactions::insert_transaction;
use tauri_app_lib::error::AppError;
use tauri_app_lib::item::cost;
use tauri_app_lib::models::{ItemInput, ItemStatus, TransactionInput};
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::common::{assert_last_error_contains, insert_account, new_account_id};
use crate::items_common::{build_input, nth_item};
use crate::world::LedgerWorld;

/// 脚手架：创建一笔 expense 购买交易并返回其 id（issue #207 起物品创建必关联
/// 购买交易，未显式 Given 交易的场景由本脚手架补齐溯源）。账户按币种惰性创建
/// 并注册到 world（与「存在账户」Given 同款）；交易经 `insert_transaction` 接缝，
/// 金额/日期/汇率不合法会在此处失败，与真实写入路径一致。
fn scaffold_purchase_tx(
    world: &mut LedgerWorld,
    date: &str,
    cost_cents: i64,
    currency: &str,
) -> String {
    let account_name = format!("物品脚手架({currency})");
    let account_id = match world.account_name_to_id.get(&account_name) {
        Some(id) => id.clone(),
        None => {
            let id = new_account_id();
            insert_account(&world.conn, &id, &account_name, "cash", currency);
            world.account_name_to_id.insert(account_name, id.clone());
            id
        }
    };
    let input = TransactionInput {
        merchant_name: None,
        kind: TransactionKind::Expense,
        amount_cents: cost_cents,
        currency_code: currency.into(),
        account_id,
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: date.into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    insert_transaction(&world.conn, input)
        .unwrap_or_else(|e| panic!("脚手架购买交易应创建成功但失败: {e}"))
}

/// 创建物品并要求成功；记录失效信号次数（写后发 `ledger:changed` 的 seam 断言）。
///
/// issue #207 起创建必关联购买交易（ADR-0025 唯一入口）：本步骤先脚手架一笔同额
/// 支出交易作为溯源再关联创建，后端以交易值覆盖带出（与本步骤入参一致，既有断言不变）。
#[when(expr = "创建物品 {string} 购买日期 {string} 总成本 {int} 币种 {string}")]
fn create_item(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
) {
    let tx_id = scaffold_purchase_tx(world, &date, cost_cents, &currency);
    let input = ItemInput {
        purchase_transaction_id: Some(tx_id),
        ..build_input(&name, date, cost_cents, &currency)
    };
    let mut signals = 0;
    let result = create_item_internal(&world.conn, input, &mut || signals += 1);
    match result {
        Ok(id) => {
            world.last_item_id = Some(id);
            world.item_signal_count = signals;
        }
        Err(AppError::Invalid(msg)) => panic!("创建物品应成功但失败: {msg}"),
        Err(e) => panic!("创建物品应成功但失败: {e}"),
    }
}

/// 创建物品（购买日期 = 今天，本地时区日历日，同 `item::cost::today` 口径）。
#[when(expr = "创建物品 {string} 今天购买 总成本 {int} 币种 {string}")]
fn create_item_bought_today(
    world: &mut LedgerWorld,
    name: String,
    cost_cents: i64,
    currency: String,
) {
    let date = cost::today().format("%Y-%m-%d").to_string();
    create_item(world, name, date, cost_cents, currency);
}

/// 创建物品（购买日期 = 今天前 N 天；N=9 → 含起止两端共 10 天）。
#[when(expr = "创建物品 {string} 今天前 {int} 天购买 总成本 {int} 币种 {string}")]
fn create_item_bought_days_ago(
    world: &mut LedgerWorld,
    name: String,
    days_ago: i64,
    cost_cents: i64,
    currency: String,
) {
    let date = (cost::today() - chrono::Duration::days(days_ago))
        .format("%Y-%m-%d")
        .to_string();
    create_item(world, name, date, cost_cents, currency);
}

/// 尝试创建物品并捕获错误（供「应返回错误」断言，与交易场景同一 seam）。
/// 同 `create_item`：先脚手架购买交易再关联创建（issue #207）。
#[when(expr = "尝试创建物品 {string} 购买日期 {string} 总成本 {int} 币种 {string}")]
fn try_create_item(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
) {
    let tx_id = scaffold_purchase_tx(world, &date, cost_cents, &currency);
    let input = ItemInput {
        purchase_transaction_id: Some(tx_id),
        ..build_input(&name, date, cost_cents, &currency)
    };
    let mut signals = 0;
    let result = create_item_internal(&world.conn, input, &mut || signals += 1);
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

/// 刷新物品列表快照并断言件数。
#[then(expr = "物品列表应包含 {int} 件物品")]
fn refresh_and_check_item_count(world: &mut LedgerWorld, expected: usize) {
    world.items_list = list_items_internal(&world.conn).expect("列出物品失败");
    assert_eq!(
        world.items_list.len(),
        expected,
        "物品件数不匹配: {:?}",
        world
            .items_list
            .iter()
            .map(|i| &i.item.name)
            .collect::<Vec<_>>()
    );
}

#[then(expr = "第 {int} 件物品名称应为 {string}")]
fn check_item_name(world: &mut LedgerWorld, n: usize, name: String) {
    assert_eq!(nth_item(world, n).item.name, name);
}

#[then(expr = "第 {int} 件物品总成本应为 {int} 币种应为 {string} 本位币成本应为 {int}")]
fn check_item_amounts(
    world: &mut LedgerWorld,
    n: usize,
    cost_cents: i64,
    currency: String,
    native_cents: i64,
) {
    let item = &nth_item(world, n).item;
    assert_eq!(item.total_cost_cents, cost_cents);
    assert_eq!(item.currency_code, currency);
    assert_eq!(item.cost_native_cents, native_cents);
}

#[then(expr = "第 {int} 件物品状态应为 {string}")]
fn check_item_status(world: &mut LedgerWorld, n: usize, status: String) {
    let parsed = ItemStatus::parse(&status).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(nth_item(world, n).item.status, parsed);
}

#[then(expr = "第 {int} 件物品已用天数应为 {int} 每天成本应为 {float}")]
fn check_item_daily_cost(world: &mut LedgerWorld, n: usize, days: i64, per_day: f64) {
    let entry = nth_item(world, n);
    assert_eq!(entry.used_days, days);
    assert!(
        (entry.per_day_cents - per_day).abs() < 1e-6,
        "每天成本不匹配: 期望 {per_day}, 实际 {}",
        entry.per_day_cents
    );
}

#[then(expr = "第 {int} 件物品应有唯一 ID 与审计字段")]
fn check_item_audit_fields(world: &mut LedgerWorld, n: usize) {
    let item = &nth_item(world, n).item;
    assert!(!item.id.is_empty(), "物品 id 不应为空");
    assert_eq!(item.version, 1, "新物品 version 应为 1");
    assert!(!item.device_id.is_empty(), "device_id 不应为空");
    assert!(!item.created_at.is_empty(), "created_at 不应为空");
    assert!(!item.updated_at.is_empty(), "updated_at 不应为空");
    assert!(!item.is_deleted);
}

#[then(expr = "写入后应发出 {int} 次失效信号")]
fn check_item_signals(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(
        world.item_signal_count, expected,
        "失效信号次数不匹配（生产路径对应 ledger:changed）"
    );
}

#[then(expr = "未发出失效信号")]
fn check_no_item_signals(world: &mut LedgerWorld) {
    assert_eq!(world.item_signal_count, 0, "不应发出失效信号");
}

/// 复用交易的「应返回错误」断言（同一 seam：world.last_error 包含片段）。
#[then(expr = "物品创建应返回错误 {string}")]
fn check_item_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}
