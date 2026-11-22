use cucumber::{then, when};

use ledger_physical_asset::{
    PhysicalAssetUpdateInput, PhysicalAssetValuationInput,
    update_physical_asset as update_physical_asset_domain,
    update_physical_asset_valuation as update_physical_asset_valuation_domain,
};

use crate::common::assert_last_error_contains;
use crate::world::LedgerWorld;

const NONE: &str = "无";

fn build_valuation_input(amount: &str, currency: &str, date: &str) -> PhysicalAssetValuationInput {
    PhysicalAssetValuationInput {
        amount_cents: amount.parse::<i64>().ok(),
        currency_code: (!currency.eq_ignore_ascii_case(NONE)).then(|| currency.to_string()),
        valuation_date: (!date.eq_ignore_ascii_case(NONE)).then(|| date.into()),
    }
}

fn build_update_input(
    name: &str,
    purchase_date: &str,
    purchase_price: &str,
    purchase_currency: &str,
) -> PhysicalAssetUpdateInput {
    let price = purchase_price.parse::<i64>().ok();
    let purchase_currency_code = (price.is_some() && !purchase_currency.eq_ignore_ascii_case(NONE))
        .then(|| purchase_currency.to_string());
    PhysicalAssetUpdateInput {
        name: name.into(),
        purchase_date: (!purchase_date.eq_ignore_ascii_case(NONE)).then(|| purchase_date.into()),
        purchase_price_cents: price,
        purchase_currency_code,
    }
}

fn require_last_asset_id(world: &LedgerWorld) -> String {
    world
        .asset
        .last_physical_asset_id
        .clone()
        .expect("更新 / 编辑前应先创建实物资产")
}

#[when(expr = "更新实物资产估值 金额 {string} 币种 {string} 估值日期 {string}")]
fn update_valuation(world: &mut LedgerWorld, amount: String, currency: String, date: String) {
    let id = require_last_asset_id(world);
    let input = build_valuation_input(&amount, &currency, &date);
    let mut signals = 0;
    match update_physical_asset_valuation_domain(&world_conn!(world), &id, input, &mut || {
        signals += 1
    }) {
        Ok(()) => world.asset.physical_asset_signal_count = signals,
        Err(e) => panic!("更新实物资产估值应成功但失败: {e}"),
    }
}

#[when(expr = "尝试更新实物资产估值 金额 {string} 币种 {string} 估值日期 {string}")]
fn try_update_valuation(world: &mut LedgerWorld, amount: String, currency: String, date: String) {
    let id = require_last_asset_id(world);
    let input = build_valuation_input(&amount, &currency, &date);
    let mut signals = 0;
    match update_physical_asset_valuation_domain(&world_conn!(world), &id, input, &mut || {
        signals += 1
    }) {
        Ok(()) => panic!("更新实物资产估值应失败但成功"),
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.asset.physical_asset_signal_count = signals;
        }
    }
}

#[when(expr = "编辑实物资产 名称 {string} 购买日期 {string} 购买价 {string} 币种 {string}")]
fn update_asset(
    world: &mut LedgerWorld,
    name: String,
    purchase_date: String,
    purchase_price: String,
    purchase_currency: String,
) {
    let id = require_last_asset_id(world);
    let input = build_update_input(&name, &purchase_date, &purchase_price, &purchase_currency);
    let mut signals = 0;
    match update_physical_asset_domain(&world_conn!(world), &id, input, &mut || signals += 1) {
        Ok(()) => world.asset.physical_asset_signal_count = signals,
        Err(e) => panic!("编辑实物资产应成功但失败: {e}"),
    }
}

#[when(expr = "尝试编辑实物资产 名称 {string} 购买日期 {string} 购买价 {string} 币种 {string}")]
fn try_update_asset(
    world: &mut LedgerWorld,
    name: String,
    purchase_date: String,
    purchase_price: String,
    purchase_currency: String,
) {
    let id = require_last_asset_id(world);
    let input = build_update_input(&name, &purchase_date, &purchase_price, &purchase_currency);
    let mut signals = 0;
    match update_physical_asset_domain(&world_conn!(world), &id, input, &mut || signals += 1) {
        Ok(()) => panic!("编辑实物资产应失败但成功"),
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.asset.physical_asset_signal_count = signals;
        }
    }
}

#[then(expr = "实物资产操作应返回错误 {string}")]
fn check_operation_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}

#[then(expr = "第 {int} 件资产版本应为 {int}")]
fn assert_asset_version(world: &mut LedgerWorld, index: usize, expected: i64) {
    let asset = &world
        .asset
        .physical_assets_list
        .as_ref()
        .expect("应先拉取列表快照")
        .assets[index - 1];
    assert_eq!(asset.version, expected, "资产版本不符");
}
