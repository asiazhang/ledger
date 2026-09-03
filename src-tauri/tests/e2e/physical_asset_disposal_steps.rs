//! 实物资产处置与软删 BDD 步骤（issue #468 T3 / spec #465 / ADR-0064）：
//! 处置守卫、处置读回、软删不可见、信号计数递增。

use cucumber::{then, when};

use tauri_app_lib::physical_asset::{
    PhysicalAssetDisposeInput, delete_physical_asset as delete_physical_asset_domain,
    dispose_physical_asset as dispose_physical_asset_domain,
    list_physical_assets as list_physical_assets_domain,
};

use crate::world::LedgerWorld;

const NONE: &str = "无";

fn build_dispose_input(date: &str, price: &str, currency: &str) -> PhysicalAssetDisposeInput {
    let price_cents = price.parse::<i64>().ok();
    let disposal_currency_code = (price_cents.is_some() && !currency.eq_ignore_ascii_case(NONE))
        .then(|| currency.to_string());
    PhysicalAssetDisposeInput {
        disposal_date: (!date.eq_ignore_ascii_case(NONE)).then(|| date.into()),
        disposal_price_cents: price_cents,
        disposal_currency_code,
    }
}

fn require_last_asset_id(world: &LedgerWorld) -> String {
    world
        .last_physical_asset_id
        .clone()
        .expect("处置 / 软删前应先创建实物资产")
}

#[when(expr = "处置实物资产 处置日期 {string} 处置价 {string} 币种 {string}")]
fn dispose_asset(world: &mut LedgerWorld, date: String, price: String, currency: String) {
    let id = require_last_asset_id(world);
    let input = build_dispose_input(&date, &price, &currency);
    let mut signals = 0;
    match dispose_physical_asset_domain(&world_conn!(world), &id, input, &mut || signals += 1) {
        Ok(()) => world.physical_asset_signal_count = signals,
        Err(e) => panic!("处置实物资产应成功但失败: {e}"),
    }
}

#[when(expr = "尝试处置实物资产 处置日期 {string} 处置价 {string} 币种 {string}")]
fn try_dispose_asset(world: &mut LedgerWorld, date: String, price: String, currency: String) {
    let id = require_last_asset_id(world);
    let input = build_dispose_input(&date, &price, &currency);
    let mut signals = 0;
    match dispose_physical_asset_domain(&world_conn!(world), &id, input, &mut || signals += 1) {
        Ok(()) => panic!("处置实物资产应失败但成功"),
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.physical_asset_signal_count = signals;
        }
    }
}

#[when(expr = "软删除实物资产")]
fn delete_asset(world: &mut LedgerWorld) {
    let id = require_last_asset_id(world);
    let mut signals = 0;
    match delete_physical_asset_domain(&world_conn!(world), &id, &mut || signals += 1) {
        Ok(()) => world.physical_asset_signal_count = signals,
        Err(e) => panic!("软删除实物资产应成功但失败: {e}"),
    }
}

#[then(expr = "已处置筛选下实物资产列表应包含 {int} 件资产")]
fn list_disposed_assets(world: &mut LedgerWorld, expected: usize) {
    let list = list_physical_assets_domain(&world_conn!(world), Some("disposed"))
        .expect("列表实物资产应成功");
    assert_eq!(
        list.assets.len(),
        expected,
        "已处置筛选列表件数不符: {:?}",
        list.assets
    );
    world.physical_assets_list = Some(list);
}

#[then(expr = "第 {int} 件资产处置日期应为 {string} 处置价应为 {int} 币种 {string}")]
fn assert_disposal_fields(
    world: &mut LedgerWorld,
    index: usize,
    date: String,
    cents: i64,
    currency: String,
) {
    let asset = &world
        .physical_assets_list
        .as_ref()
        .expect("应先拉取列表快照")
        .assets[index - 1];
    assert_eq!(
        asset.disposal_date.as_deref(),
        Some(date.as_str()),
        "处置日期不符"
    );
    assert_eq!(asset.disposal_price_cents, Some(cents), "处置价不符");
    assert_eq!(
        asset.disposal_currency_code.as_deref(),
        Some(currency.as_str()),
        "处置币种不符"
    );
}

#[then(expr = "第 {int} 件资产当前估值折本位币应为空")]
fn assert_native_valuation_none(world: &mut LedgerWorld, index: usize) {
    let asset = &world
        .physical_assets_list
        .as_ref()
        .expect("应先拉取列表快照")
        .assets[index - 1];
    assert!(
        asset.current_valuation_native_cents.is_none(),
        "已处置行不应折本位币"
    );
}

#[then(expr = "已软删资产数据与估值历史应保留")]
fn assert_soft_deleted_data_preserved(world: &mut LedgerWorld) {
    let id = require_last_asset_id(world);
    let conn = world_conn!(world);
    let is_deleted: i64 = conn
        .query_row(
            "SELECT is_deleted FROM physical_assets WHERE id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .expect("软删后资产行应保留");
    assert_eq!(is_deleted, 1, "资产行应带软删标志保留");
    let valuation_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM physical_asset_valuations WHERE asset_id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .expect("软删后估值历史应保留");
    assert!(valuation_count >= 1, "估值历史行不应随软删消失");
}
