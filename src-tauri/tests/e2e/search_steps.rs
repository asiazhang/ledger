use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::commands::search::{rebuild_search_index, search_transactions_internal};
use tauri_app_lib::db::{device_id, new_uuid, now_iso};
use tauri_app_lib::models::TransactionSearchResult;

use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

/// 存量交易：直接 SQL 插入，绕过应用层索引钩子（模拟 V005 迁移前的存量数据）。
#[given(expr = "存量交易 备注 {string} 金额 {int} 账户 {string} 日期 {string}")]
fn legacy_txn(
    world: &mut LedgerWorld,
    note: String,
    amount: i64,
    account_name: String,
    date: String,
) {
    let account_id = world.account_id(&account_name);
    let id = new_uuid();
    let now = now_iso();
    world
        .conn
        .execute(
            "INSERT INTO transactions \
             (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
             category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'expense',?2,'CNY',?2,?3,NULL,NULL,NULL,?4,?5,?6,?6,1,?7,0)",
            params![id, amount, account_id, note, date, now, device_id()],
        )
        .unwrap();
    world.last_transaction_id = Some(id);
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "重建搜索索引")]
fn rebuild_index(world: &mut LedgerWorld) {
    rebuild_search_index(&world.conn).expect("重建搜索索引失败");
}

#[when(expr = "搜索 {string}")]
fn search(world: &mut LedgerWorld, query: String) {
    world.last_search =
        Some(search_transactions_internal(&world.conn, &query, 1, 20).expect("搜索失败"));
}

#[when(expr = "搜索 {string} 第 {int} 页 每页 {int} 条")]
fn search_paged(world: &mut LedgerWorld, query: String, page: usize, page_size: usize) {
    world.last_search =
        Some(search_transactions_internal(&world.conn, &query, page, page_size).expect("搜索失败"));
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

fn search_snapshot(world: &LedgerWorld) -> &TransactionSearchResult {
    world.last_search.as_ref().expect("尚未执行搜索")
}

#[then(expr = "搜索命中 {int} 条")]
fn search_hits(world: &mut LedgerWorld, expected: usize) {
    let snapshot = search_snapshot(world);
    assert_eq!(
        snapshot.items.len(),
        expected,
        "搜索结果当前页条数不匹配（total={}）",
        snapshot.total
    );
}

#[then(expr = "搜索命中 {int} 条 总数 {int}")]
fn search_hits_total(world: &mut LedgerWorld, expected_items: usize, expected_total: i64) {
    let snapshot = search_snapshot(world);
    assert_eq!(
        snapshot.items.len(),
        expected_items,
        "搜索结果当前页条数不匹配"
    );
    assert_eq!(snapshot.total, expected_total, "命中总数不匹配");
}

#[then(expr = "搜索结果第 {int} 条备注应为 {string}")]
fn search_nth_note(world: &mut LedgerWorld, index: i64, expected: String) {
    let snapshot = search_snapshot(world);
    let item = snapshot
        .items
        .get((index - 1) as usize)
        .unwrap_or_else(|| panic!("搜索结果不足 {} 条", index));
    assert_eq!(
        item.note.as_deref(),
        Some(expected.as_str()),
        "第 {index} 条搜索结果备注不匹配"
    );
}

#[then(expr = "搜索结果第 {int} 条金额应为 {int}")]
fn search_nth_amount(world: &mut LedgerWorld, index: i64, expected: i64) {
    let snapshot = search_snapshot(world);
    let item = snapshot
        .items
        .get((index - 1) as usize)
        .unwrap_or_else(|| panic!("搜索结果不足 {} 条", index));
    assert_eq!(
        item.amount_cents, expected,
        "第 {index} 条搜索结果金额不匹配"
    );
}
