//! 标的字典步骤（issue #199）：搜索语义的 BDD 接缝。实现为
//! `commands::investment::list_instruments_internal`（与 IPC 命令同一实现）。

use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::commands::investment::list_instruments_internal;
use tauri_app_lib::db::{device_id, new_uuid, now_iso};
use tauri_app_lib::models::InstrumentListFilter;

use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

/// 直接插入金融工具字典行（投资域字典，可指定中文名称供拼音语义场景使用）。
#[given(expr = "存在标的 {string} 名称 {string} 币种 {string}")]
fn create_instrument_named(
    world: &mut LedgerWorld,
    symbol: String,
    name: String,
    currency: String,
) {
    let now = now_iso();
    world_conn!(world)
        .execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,'stock',?3,?4,'unknown',?5,?5,1,?6)",
            params![new_uuid(), symbol, name, currency, now, device_id()],
        )
        .unwrap();
}

/// 直接插入指定类型的金融工具字典行（同码异类型消歧场景用，issue #294）。
#[given(expr = "存在类型 {string} 的标的 {string} 名称 {string} 币种 {string}")]
fn create_instrument_of_type(
    world: &mut LedgerWorld,
    kind: String,
    symbol: String,
    name: String,
    currency: String,
) {
    let now = now_iso();
    world_conn!(world)
        .execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,?3,?4,?5,'unknown',?6,?6,1,?7)",
            params![new_uuid(), symbol, kind, name, currency, now, device_id()],
        )
        .unwrap();
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "搜索标的 {string}")]
fn search_instruments(world: &mut LedgerWorld, query: String) {
    let filter = InstrumentListFilter {
        search: Some(query),
        ..Default::default()
    };
    world.last_instrument_search =
        Some(list_instruments_internal(&world_conn!(world), &filter).expect("标的搜索失败"));
}

/// 按类型过滤搜索（同码异类型消歧语义，issue #294；与 HTTP 端点的 type 参数同一接缝）。
#[when(expr = "搜索类型 {string} 的标的 {string}")]
fn search_instruments_of_kind(world: &mut LedgerWorld, kind: String, query: String) {
    let filter = InstrumentListFilter {
        search: Some(query),
        kind: Some(kind.parse().expect("未知金融工具类型")),
        ..Default::default()
    };
    world.last_instrument_search =
        Some(list_instruments_internal(&world_conn!(world), &filter).expect("标的搜索失败"));
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "标的搜索命中 {int} 条 总数 {int}")]
fn assert_instrument_search(world: &mut LedgerWorld, items: usize, total: i64) {
    let result = world
        .last_instrument_search
        .as_ref()
        .expect("未执行标的搜索");
    assert_eq!(result.items.len(), items, "命中条数不符：{result:?}");
    assert_eq!(result.total, total, "命中总数不符：{result:?}");
}

#[then(expr = "标的搜索首个结果代码为 {string}")]
fn assert_instrument_first_symbol(world: &mut LedgerWorld, symbol: String) {
    let result = world
        .last_instrument_search
        .as_ref()
        .expect("未执行标的搜索");
    assert_eq!(
        result.items.first().map(|i| i.symbol.as_str()),
        Some(symbol.as_str()),
        "首个结果代码不符：{result:?}"
    );
}
