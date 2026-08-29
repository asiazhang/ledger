use cucumber::{given, then, when};
use rusqlite::params;

use crate::world::LedgerWorld;
use tauri_app_lib::commands::search::search_transactions_internal;
use tauri_app_lib::db::{device_id, new_uuid, now_iso};
use tauri_app_lib::models::TransactionSearchResult;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

/// 存量交易：直接 SQL 插入，绕过应用层写入路径（语义与正常写入一致——
/// 搜索无索引，两种来源的写入立即可搜）。
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
    world_conn!(world)
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

#[when(expr = "搜索 {string}")]
fn search(world: &mut LedgerWorld, query: String) {
    world.last_search = Some(
        search_transactions_internal(&world_conn!(world), &query, 1, 20, None, None, None, None)
            .expect("搜索失败"),
    );
}

#[when(expr = "搜索 {string} 第 {int} 页 每页 {int} 条")]
fn search_paged(world: &mut LedgerWorld, query: String, page: usize, page_size: usize) {
    world.last_search = Some(
        search_transactions_internal(
            &world_conn!(world),
            &query,
            page,
            page_size,
            None,
            None,
            None,
            None,
        )
        .expect("搜索失败"),
    );
}

/// 关键字 + 金额区间（分）AND 组合。
#[when(expr = "搜索 {string} 金额区间 {int} 至 {int} 分")]
fn search_keyword_amount_range(world: &mut LedgerWorld, query: String, min: i64, max: i64) {
    world.last_search = Some(
        search_transactions_internal(
            &world_conn!(world),
            &query,
            1,
            20,
            Some(min),
            Some(max),
            None,
            None,
        )
        .expect("搜索失败"),
    );
}

/// 关键字 + 日期区间（含边界）AND 组合。
#[when(expr = "搜索 {string} 日期区间 {string} 至 {string}")]
fn search_keyword_date_range(world: &mut LedgerWorld, query: String, from: String, to: String) {
    world.last_search = Some(
        search_transactions_internal(
            &world_conn!(world),
            &query,
            1,
            20,
            None,
            None,
            Some(from.as_str()),
            Some(to.as_str()),
        )
        .expect("搜索失败"),
    );
}

/// 仅金额筛选（无关键字）：金额区间（分，含边界）。
#[when(expr = "搜索金额区间 {int} 至 {int} 分")]
fn search_amount_range(world: &mut LedgerWorld, min: i64, max: i64) {
    world.last_search = Some(
        search_transactions_internal(
            &world_conn!(world),
            "",
            1,
            20,
            Some(min),
            Some(max),
            None,
            None,
        )
        .expect("搜索失败"),
    );
}

/// 仅金额筛选（无关键字）：金额区间（元，支持小数，元→分四舍五入）。
#[when(expr = "搜索金额区间 {float} 至 {float} 元")]
fn search_amount_range_yuan(world: &mut LedgerWorld, min: f64, max: f64) {
    let min_cents = (min * 100.0).round() as i64;
    let max_cents = (max * 100.0).round() as i64;
    world.last_search = Some(
        search_transactions_internal(
            &world_conn!(world),
            "",
            1,
            20,
            Some(min_cents),
            Some(max_cents),
            None,
            None,
        )
        .expect("搜索失败"),
    );
}

/// 仅金额筛选（无关键字）：单边下限（分，含边界）。
#[when(expr = "搜索金额下限 {int} 分")]
fn search_amount_min(world: &mut LedgerWorld, min: i64) {
    world.last_search = Some(
        search_transactions_internal(&world_conn!(world), "", 1, 20, Some(min), None, None, None)
            .expect("搜索失败"),
    );
}

/// 仅金额筛选（无关键字）：单边上限（分，含边界）。
#[when(expr = "搜索金额上限 {int} 分")]
fn search_amount_max(world: &mut LedgerWorld, max: i64) {
    world.last_search = Some(
        search_transactions_internal(&world_conn!(world), "", 1, 20, None, Some(max), None, None)
            .expect("搜索失败"),
    );
}

/// 仅日期筛选（无关键字）：日期区间（含边界）。
#[when(expr = "搜索日期区间 {string} 至 {string}")]
fn search_date_range(world: &mut LedgerWorld, from: String, to: String) {
    world.last_search = Some(
        search_transactions_internal(
            &world_conn!(world),
            "",
            1,
            20,
            None,
            None,
            Some(from.as_str()),
            Some(to.as_str()),
        )
        .expect("搜索失败"),
    );
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

/// 搜索结果展示商户：按名称解析为 id 与交易的 merchant_id 比对
/// （展示名称本身由前端 merchantMap 负责交易列表信息口径，这里断言关联正确）。
#[then(expr = "搜索结果第 {int} 条商户应为 {string}")]
fn search_nth_merchant(world: &mut LedgerWorld, index: i64, expected: String) {
    let snapshot = search_snapshot(world);
    let item = snapshot
        .items
        .get((index - 1) as usize)
        .unwrap_or_else(|| panic!("搜索结果不足 {} 条", index));
    let expected_id = world.merchant_id(&expected);
    assert_eq!(
        item.merchant_id.as_deref(),
        Some(expected_id.as_str()),
        "第 {index} 条搜索结果商户不匹配"
    );
}
