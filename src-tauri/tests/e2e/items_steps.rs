//! 物品（Item）BDD 步骤（issue #115 / #118 / spec #113）：创建、列出与软删除物品。
//!
//! 经 `commands::item` 的 `*_internal` seam 断言外部可观察行为：
//! 创建读回、每天成本（`item::cost` 口径）、金额折算、写后发失效信号
//! （notify 注入，生产路径发 `ledger:changed`）、软删除后标准列表过滤。

use cucumber::{then, when};
use rusqlite::params;
use tauri_app_lib::commands::item::{
    calculate_item_cost_internal, create_item_internal, delete_item_internal,
    dispose_item_internal, item_daily_total_internal, list_items_internal, update_item_internal,
};
use tauri_app_lib::commands::transactions::insert_transaction;
use tauri_app_lib::error::AppError;
use tauri_app_lib::item::cost;
use tauri_app_lib::models::{
    ItemDailyCost, ItemDisposeInput, ItemInput, ItemStatus, TransactionInput,
};
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::common::assert_last_error_contains;
use crate::world::LedgerWorld;

/// 填备注：空字符串规为清除（None），其余原样。
fn with_note(mut input: ItemInput, note: &str) -> ItemInput {
    input.note = if note.is_empty() {
        None
    } else {
        Some(note.to_string())
    };
    input
}

fn build_input(name: &str, date: String, cost_cents: i64, currency: &str) -> ItemInput {
    ItemInput {
        name: name.into(),
        purchase_date: date,
        total_cost_cents: cost_cents,
        currency_code: currency.into(),
        note: None,
        purchase_transaction_id: None,
    }
}

/// 关联购买交易的入参：日期/成本/币种填入**故意错误的占位值**，
/// 断言「自动带出」这一外部行为（后端必须用交易值覆盖占位值）。
fn build_linked_input(name: &str, tx_id: &str) -> ItemInput {
    ItemInput {
        name: name.into(),
        purchase_date: "1970-01-01".into(),
        total_cost_cents: 1,
        currency_code: "CNY".into(),
        note: None,
        purchase_transaction_id: Some(tx_id.into()),
    }
}

/// 创建物品并要求成功；记录失效信号次数（写后发 `ledger:changed` 的 seam 断言）。
#[when(expr = "创建物品 {string} 购买日期 {string} 总成本 {int} 币种 {string}")]
fn create_item(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
) {
    let mut signals = 0;
    let result = create_item_internal(
        &world.conn,
        build_input(&name, date, cost_cents, &currency),
        &mut || signals += 1,
    );
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
#[when(expr = "尝试创建物品 {string} 购买日期 {string} 总成本 {int} 币种 {string}")]
fn try_create_item(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
) {
    let mut signals = 0;
    let result = create_item_internal(
        &world.conn,
        build_input(&name, date, cost_cents, &currency),
        &mut || signals += 1,
    );
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

/// 修改最近创建的物品（`world.last_item_id`）并要求成功；备注空字符串规为清除（None）。
#[when(
    expr = "修改物品名称为 {string} 购买日期 {string} 总成本 {int} 币种 {string} 备注为 {string}"
)]
fn update_item(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
    note: String,
) {
    let mut signals = 0;
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可修改"));
    let result = update_item_internal(
        &world.conn,
        &id,
        with_note(build_input(&name, date, cost_cents, &currency), &note),
        &mut || signals += 1,
    );
    match result {
        Ok(()) => world.item_signal_count = signals,
        Err(e) => panic!("修改物品应成功但失败: {e}"),
    }
}

/// 修改物品（购买日期 = 今天前 N 天；日期口径同创建步骤，保证天数可静态断言）。
#[when(
    expr = "修改物品名称为 {string} 今天前 {int} 天购买 总成本 {int} 币种 {string} 备注为 {string}"
)]
fn update_item_days_ago(
    world: &mut LedgerWorld,
    name: String,
    days_ago: i64,
    cost_cents: i64,
    currency: String,
    note: String,
) {
    let date = (cost::today() - chrono::Duration::days(days_ago))
        .format("%Y-%m-%d")
        .to_string();
    update_item(world, name, date, cost_cents, currency, note);
}

/// 尝试修改物品并捕获错误（供「应返回错误」断言，与创建场景同一 seam）。
#[when(
    expr = "尝试修改物品名称为 {string} 购买日期 {string} 总成本 {int} 币种 {string} 备注为 {string}"
)]
fn try_update_item(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
    note: String,
) {
    let mut signals = 0;
    // 不存在场景传固定假 id，真实走到 query_one 落空的 NotFound 路径
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| "no-such-item".into());
    let result = update_item_internal(
        &world.conn,
        &id,
        with_note(build_input(&name, date, cost_cents, &currency), &note),
        &mut || signals += 1,
    );
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
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

/// 取第 n 件（1 起）物品快照的辅助。
fn nth_item(world: &LedgerWorld, n: usize) -> &tauri_app_lib::models::ItemWithDailyCost {
    world
        .items_list
        .get(n - 1)
        .unwrap_or_else(|| panic!("物品列表第 {n} 件不存在"))
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

#[then(expr = "第 {int} 件物品版本应为 {int}")]
fn check_item_version(world: &mut LedgerWorld, n: usize, version: i64) {
    assert_eq!(nth_item(world, n).item.version, version);
}

#[then(expr = "第 {int} 件物品购买日期应为 {string}")]
fn check_item_purchase_date(world: &mut LedgerWorld, n: usize, date: String) {
    assert_eq!(nth_item(world, n).item.purchase_date, date);
}

#[then(expr = "第 {int} 件物品备注应为 {string}")]
fn check_item_note(world: &mut LedgerWorld, n: usize, note: String) {
    assert_eq!(nth_item(world, n).item.note.as_deref(), Some(note.as_str()));
}

#[then(expr = "第 {int} 件物品备注应为空")]
fn check_item_note_empty(world: &mut LedgerWorld, n: usize) {
    assert_eq!(nth_item(world, n).item.note, None);
}

#[when(expr = "记住第 {int} 件物品的创建时间")]
fn remember_item_created_at(world: &mut LedgerWorld, n: usize) {
    world.items_list = list_items_internal(&world.conn).expect("列出物品失败");
    world.remembered_item_created_at = Some(nth_item(world, n).item.created_at.clone());
}

#[then(expr = "第 {int} 件物品创建时间应与记住的一致")]
fn check_item_created_at_preserved(world: &mut LedgerWorld, n: usize) {
    let remembered = world
        .remembered_item_created_at
        .as_deref()
        .unwrap_or_else(|| panic!("没有记住的创建时间（先调「记住…创建时间」步骤）"));
    assert_eq!(
        nth_item(world, n).item.created_at,
        remembered,
        "修改不应改动 created_at"
    );
}

/// 成本分解断言：分子 ÷ 天数 = 每天成本（详情视图展示的口径三元组）。
#[then(expr = "第 {int} 件物品成本分解分子应为 {int} 分 ÷ {int} 天 = 每天成本 {float}")]
fn check_item_cost_breakdown(
    world: &mut LedgerWorld,
    n: usize,
    numerator: i64,
    days: i64,
    per_day: f64,
) {
    let entry = nth_item(world, n);
    assert_eq!(entry.numerator_cents, numerator, "成本分解分子不匹配");
    assert_eq!(entry.used_days, days, "成本分解天数不匹配");
    assert!(
        (entry.per_day_cents - per_day).abs() < 1e-6,
        "每天成本不匹配: 期望 {per_day}, 实际 {}",
        entry.per_day_cents
    );
}

#[then(expr = "物品修改应返回错误 {string}")]
fn check_item_update_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
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

/// 按名称查未删除物品 id 的辅助（失败即 panic，场景数据自洽由写步骤保证）。
fn find_item_id_by_name(conn: &rusqlite::Connection, name: &str) -> String {
    conn.query_row(
        "SELECT id FROM items WHERE name=?1 AND is_deleted=0",
        rusqlite::params![name],
        |r| r.get(0),
    )
    .unwrap_or_else(|e| panic!("按名称 {name} 查找未删除物品失败: {e}"))
}

/// 软删除指定名称的物品（要求成功；记录失效信号次数）。
#[when(expr = "软删除物品 {string}")]
fn soft_delete_item(world: &mut LedgerWorld, name: String) {
    let id = find_item_id_by_name(&world.conn, &name);
    let mut signals = 0;
    let result = delete_item_internal(&world.conn, &id, &mut || signals += 1);
    world.item_signal_count = signals;
    if let Err(e) = result {
        panic!("软删除物品 {name} 应成功但失败: {e}");
    }
}

#[then(expr = "删除后应发出 {int} 次失效信号")]
fn check_item_delete_signals(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(
        world.item_signal_count, expected,
        "删除失效信号次数不匹配（生产路径对应 ledger:changed）"
    );
}

/// 直接查库断言软删除语义：行未被物理移除，仅打 `is_deleted=1` 标记。
#[then(expr = "物品 {string} 行仍存在且 is_deleted=1")]
fn check_item_row_soft_deleted(world: &mut LedgerWorld, name: String) {
    let (count, is_deleted): (i64, i64) = world
        .conn
        .query_row(
            "SELECT COUNT(*), MAX(is_deleted) FROM items WHERE name=?1",
            rusqlite::params![name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("查询物品行失败");
    assert_eq!(count, 1, "软删除后行应保留在库（不物理移除）");
    assert_eq!(is_deleted, 1, "软删除应打 is_deleted=1 标记");
}

/// 尝试删除不存在的物品 id（捕获错误供「应返回错误」断言）。
#[when(expr = "尝试软删除不存在的物品")]
fn try_delete_missing_item(world: &mut LedgerWorld) {
    let mut signals = 0;
    let result = delete_item_internal(&world.conn, "no-such-item-id", &mut || signals += 1);
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
    };
}

#[then(expr = "物品删除应返回错误 {string}")]
fn check_item_delete_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}

// ---------------------------------------------------------------------------
// 关联购买交易（issue #119）：自动带出日期/成本，存溯源，校验存在且为 expense
// ---------------------------------------------------------------------------

/// 创建一笔外币支出交易（通用「创建交易」步骤固定 CNY，此处补币种参数）。
#[when(expr = "创建支出交易 金额 {int} 币种 {string} 到账户 {string} 日期 {string}")]
fn create_expense_txn_with_currency(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account_name: String,
    date: String,
) {
    let input = TransactionInput {
        kind: TransactionKind::Expense,
        amount_cents: amount,
        currency_code: currency,
        account_id: world.account_id(&account_name),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date,
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let result = insert_transaction(&world.conn, input);
    let id = result.unwrap_or_else(|e| panic!("创建支出交易应成功但失败: {e}"));
    world.last_transaction_id = Some(id);
}

/// 记住最近创建的交易为关联购买交易（后续「关联该购买交易」步骤引用它）。
#[when(expr = "记住该交易为关联购买交易")]
fn remember_purchase_transaction(world: &mut LedgerWorld) {
    world.remembered_purchase_transaction_id = world.last_transaction_id.clone();
}

/// 创建物品并关联记住的购买交易：入参日期/成本为占位值，
/// 后端必须用交易值覆盖（自动带出）。
#[when(expr = "创建物品 {string} 关联该购买交易")]
fn create_item_linked(world: &mut LedgerWorld, name: String) {
    let tx_id = world
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易（先调「记住该交易为关联购买交易」）"));
    let mut signals = 0;
    let result = create_item_internal(&world.conn, build_linked_input(&name, &tx_id), &mut || {
        signals += 1
    });
    match result {
        Ok(id) => {
            world.last_item_id = Some(id);
            world.item_signal_count = signals;
        }
        Err(e) => panic!("创建物品应成功但失败: {e}"),
    }
}

/// 尝试创建关联记住交易的物品并捕获错误（非 expense 报错路径）。
#[when(expr = "尝试创建物品 {string} 关联该购买交易")]
fn try_create_item_linked(world: &mut LedgerWorld, name: String) {
    let tx_id = world
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易（先调「记住该交易为关联购买交易」）"));
    let mut signals = 0;
    let result = create_item_internal(&world.conn, build_linked_input(&name, &tx_id), &mut || {
        signals += 1
    });
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

/// 尝试创建关联不存在交易的物品并捕获错误（固定假 id 走不存在报错路径）。
#[when(expr = "尝试创建物品 {string} 关联不存在的购买交易")]
fn try_create_item_linked_missing(world: &mut LedgerWorld, name: String) {
    let mut signals = 0;
    let result = create_item_internal(
        &world.conn,
        build_linked_input(&name, "no-such-transaction"),
        &mut || signals += 1,
    );
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

/// 修改最近创建的物品并关联记住的购买交易。入参日期/成本为占位值：
/// 新关联/换关时后端必须用交易值覆盖（自动带出）；维持既有关联时则原样落库，
/// 两种语义由不同场景分别断言。
#[when(
    expr = "修改物品名称为 {string} 购买日期 {string} 总成本 {int} 币种 {string} 关联该购买交易 备注为 {string}"
)]
fn update_item_linked(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
    note: String,
) {
    let tx_id = world
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易（先调「记住该交易为关联购买交易」）"));
    let mut signals = 0;
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可修改"));
    let mut input = build_linked_input(&name, &tx_id);
    input.purchase_date = date;
    input.total_cost_cents = cost_cents;
    input.currency_code = currency;
    input.note = if note.is_empty() { None } else { Some(note) };
    let result = update_item_internal(&world.conn, &id, input, &mut || signals += 1);
    match result {
        Ok(()) => world.item_signal_count = signals,
        Err(e) => panic!("修改物品应成功但失败: {e}"),
    }
}

/// 尝试修改最近创建的物品并关联记住的购买交易（捕获错误，溯源唯一拒绝路径）。
#[when(
    expr = "尝试修改物品名称为 {string} 购买日期 {string} 总成本 {int} 币种 {string} 关联该购买交易 备注为 {string}"
)]
fn try_update_item_linked(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
    note: String,
) {
    let tx_id = world
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易"));
    let mut signals = 0;
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可修改"));
    let mut input = build_linked_input(&name, &tx_id);
    input.purchase_date = date;
    input.total_cost_cents = cost_cents;
    input.currency_code = currency;
    input.note = if note.is_empty() { None } else { Some(note) };
    let result = update_item_internal(&world.conn, &id, input, &mut || signals += 1);
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
    };
}

/// 断言第 n 件物品的溯源指向记住的关联购买交易。
#[then(expr = "第 {int} 件物品关联购买交易应为记住的交易")]
fn check_item_linked_transaction(world: &mut LedgerWorld, n: usize) {
    let expected = world
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易"));
    assert_eq!(
        nth_item(world, n).item.purchase_transaction_id.as_deref(),
        Some(expected.as_str()),
        "物品溯源应指向记住的关联购买交易"
    );
}

/// 处置物品的共用入口：记录失效信号次数并返回结果（成功/错误均不 panic）。
fn dispose_by_id(
    world: &mut LedgerWorld,
    id: &str,
    input: ItemDisposeInput,
) -> Result<(), tauri_app_lib::error::AppError> {
    let mut signals = 0;
    let result = dispose_item_internal(&world.conn, id, input, &mut || signals += 1);
    world.item_signal_count = signals;
    result
}

/// 处置最近创建的物品（`world.last_item_id`），要求成功。
#[when(expr = "处置物品 处置日期 {string} 残值 {int}")]
fn dispose_item_with_residual(world: &mut LedgerWorld, date: String, residual: i64) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可处置"));
    assert_dispose_ok(world, &id, date, Some(residual));
}

/// 处置最近创建的物品，不填残值（残值可选语义）。
#[when(expr = "处置物品 处置日期 {string} 不填残值")]
fn dispose_item_without_residual(world: &mut LedgerWorld, date: String) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可处置"));
    assert_dispose_ok(world, &id, date, None);
}

fn assert_dispose_ok(world: &mut LedgerWorld, id: &str, date: String, residual: Option<i64>) {
    if let Err(e) = dispose_by_id(
        world,
        id,
        ItemDisposeInput {
            disposal_date: date,
            residual_value_cents: residual,
        },
    ) {
        panic!("处置物品应成功但失败: {e}");
    }
}

/// 尝试处置最近创建的物品并捕获错误（供「应返回错误」断言）。
#[when(expr = "尝试处置物品 处置日期 {string} 残值 {int}")]
fn try_dispose_item(world: &mut LedgerWorld, date: String, residual: i64) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| "no-such-item".into());
    world.last_error = match dispose_by_id(
        world,
        &id,
        ItemDisposeInput {
            disposal_date: date,
            residual_value_cents: Some(residual),
        },
    ) {
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
    };
}

/// 尝试处置不存在的物品 id（固定假 id 走 NotFound 报错路径）。
#[when(expr = "尝试处置不存在的物品")]
fn try_dispose_missing_item(world: &mut LedgerWorld) {
    world.last_error = match dispose_by_id(
        world,
        "no-such-item-id",
        ItemDisposeInput {
            disposal_date: "2026-01-01".into(),
            residual_value_cents: None,
        },
    ) {
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
    };
}

/// 断言第 n 件物品的处置日期与残值读回。
#[then(expr = "第 {int} 件物品处置日期应为 {string} 残值应为 {int}")]
fn check_item_disposal(world: &mut LedgerWorld, n: usize, date: String, residual: i64) {
    let item = &nth_item(world, n).item;
    assert_eq!(item.disposal_date.as_deref(), Some(date.as_str()));
    assert_eq!(item.residual_value_cents, Some(residual));
}

/// 断言第 n 件物品处置日期读回且残值为空（可选残值语义）。
#[then(expr = "第 {int} 件物品处置日期应为 {string} 残值应为空")]
fn check_item_disposal_no_residual(world: &mut LedgerWorld, n: usize, date: String) {
    let item = &nth_item(world, n).item;
    assert_eq!(item.disposal_date.as_deref(), Some(date.as_str()));
    assert_eq!(item.residual_value_cents, None);
}

/// 复用「应返回错误」断言（同一 seam：world.last_error 包含片段）。
#[then(expr = "物品处置应返回错误 {string}")]
fn check_item_dispose_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}

// ---------------------------------------------------------------------------
// 自选参考日重算（issue #121）：计算接口接受可选参考日，缺省沿用列表口径
// ---------------------------------------------------------------------------

/// 计算最近创建的物品（`world.last_item_id`）每天使用成本的共用入口。
fn calc_item_cost(
    world: &mut LedgerWorld,
    id: &str,
    reference_date: Option<String>,
) -> Result<ItemDailyCost, AppError> {
    calculate_item_cost_internal(&world.conn, id, reference_date.as_deref())
}

/// 缺省参考日（不传）：在用 → 今天；已处置 → 处置日（口径与列表一致）。
#[when(expr = "按最近创建的物品计算每天成本 不带参考日")]
fn calc_item_cost_default(world: &mut LedgerWorld) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可计算"));
    world.last_item_cost = Some(calc_item_cost(world, &id, None).expect("计算每天成本应成功"));
}

/// 自选参考日 = 今天前 N 天（相对日期，保证天数可静态断言）。
#[when(expr = "按最近创建的物品计算每天成本 今天前 {int} 天为参考日")]
fn calc_item_cost_days_ago(world: &mut LedgerWorld, days_ago: i64) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可计算"));
    let date = (cost::today() - chrono::Duration::days(days_ago))
        .format("%Y-%m-%d")
        .to_string();
    world.last_item_cost =
        Some(calc_item_cost(world, &id, Some(date)).expect("计算每天成本应成功"));
}

/// 自选参考日 = 今天后 N 天（预览「用满 N 天」的摊薄）。
#[when(expr = "按最近创建的物品计算每天成本 今天后 {int} 天为参考日")]
fn calc_item_cost_days_later(world: &mut LedgerWorld, days_later: i64) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可计算"));
    let date = (cost::today() + chrono::Duration::days(days_later))
        .format("%Y-%m-%d")
        .to_string();
    world.last_item_cost =
        Some(calc_item_cost(world, &id, Some(date)).expect("计算每天成本应成功"));
}

/// 自选固定参考日（YYYY-MM-DD）。
#[when(expr = "按最近创建的物品计算每天成本 参考日 {string}")]
fn calc_item_cost_fixed_ref(world: &mut LedgerWorld, date: String) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可计算"));
    world.last_item_cost =
        Some(calc_item_cost(world, &id, Some(date)).expect("计算每天成本应成功"));
}

/// 尝试按指定参考日计算并捕获错误（供「应返回错误」断言）。
#[when(expr = "尝试按最近创建的物品计算每天成本 参考日 {string}")]
fn try_calc_item_cost(world: &mut LedgerWorld, date: String) {
    // 不存在场景传固定假 id，真实走到 query_one 落空的 NotFound 路径（同其它步骤惯例）
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| "no-such-item-id".into());
    world.last_error = match calc_item_cost(world, &id, Some(date)) {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

/// 尝试计算不存在的物品 id（固定假 id 走 NotFound 报错路径）。
#[when(expr = "尝试按不存在的物品计算每天成本")]
fn try_calc_item_cost_missing(world: &mut LedgerWorld) {
    world.last_error = match calc_item_cost(world, "no-such-item-id", None) {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

/// 断言重算结果三元组：分子 ÷ 天数 = 每天成本（与详情视图展示口径一致）。
#[then(expr = "计算结果已用天数应为 {int} 分子应为 {int} 每天成本应为 {float}")]
fn check_calc_item_cost(world: &mut LedgerWorld, days: i64, numerator: i64, per_day: f64) {
    let result = world
        .last_item_cost
        .as_ref()
        .unwrap_or_else(|| panic!("没有计算结果（先调「按最近创建的物品计算每天成本」）"));
    assert_eq!(result.used_days, days, "重算天数不匹配");
    assert_eq!(result.numerator_cents, numerator, "重算分子不匹配");
    assert!(
        (result.per_day_cents - per_day).abs() < 1e-6,
        "重算每天成本不匹配: 期望 {per_day}, 实际 {}",
        result.per_day_cents
    );
}

#[then(expr = "计算每天成本应返回错误 {string}")]
fn check_calc_item_cost_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}

// ---------------------------------------------------------------------------
// dashboard 汇总卡聚合（issue #122）：全部在用物品每天成本合计（本位币）
// ---------------------------------------------------------------------------

/// 查询全部在用物品每天成本合计（错误路径记入 last_error，供「应返回错误」断言）。
#[when(expr = "查询在用物品每天成本合计")]
fn query_item_daily_total(world: &mut LedgerWorld) {
    match item_daily_total_internal(&world.conn) {
        Ok(total) => {
            world.last_item_daily_total = Some(total);
            world.last_error = None;
        }
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.last_item_daily_total = None;
        }
    }
}

/// 断言合计三元组：每天成本合计（本位币分/天）+ 默认币种代码 + 计入件数。
#[then(expr = "在用物品每天成本合计应为 {float} 本位币应为 {string} 件数应为 {int}")]
fn check_item_daily_total(world: &mut LedgerWorld, per_day: f64, currency: String, count: usize) {
    let total = world
        .last_item_daily_total
        .as_ref()
        .expect("未查询到合计（先调「查询在用物品每天成本合计」）");
    assert!(
        (total.per_day_cents - per_day).abs() < 1e-6,
        "每天成本合计不匹配: 期望 {per_day}, 实际 {}",
        total.per_day_cents
    );
    assert_eq!(total.native_currency, currency, "合计币种应为默认币种");
    assert_eq!(total.item_count, count as u64, "计入合计的件数不匹配");
}

/// 移除汇率行（测试脚手架，与 scheduled_steps 的「存在汇率」对偶）：
/// 构造「物品落库时有汇率、聚合时缺汇率」的环境，断言错误上抛而非以零计入。
#[when(expr = "移除汇率 {string} 兑 {string}")]
fn remove_exchange_rate(world: &mut LedgerWorld, base: String, quote: String) {
    world
        .conn
        .execute(
            "DELETE FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
            params![base, quote],
        )
        .unwrap();
}
