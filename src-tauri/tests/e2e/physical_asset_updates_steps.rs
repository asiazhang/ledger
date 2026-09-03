//! 实物资产更新估值与编辑档案 BDD 步骤（issue #467 T2 / spec #465 / ADR-0064）：
//! 更新估值（追加历史行，当前估值 = 最新一条）、编辑档案（名称 / 购买信息，
//! 无估值字段）的守卫与读回、失效信号计数。
//!
//! 经 `physical_asset` 域 API（`tauri_app_lib::physical_asset`）断言外部可观察
//! 行为；定位「最近创建的资产」复用 world 的 `last_physical_asset_id`（T1 步骤
//! 写入，跨步骤读写状态）；「应返回错误」「失效信号」断言语义与 T1 同源，
//! 仅步骤措辞面向 T2 操作（cucumber 表达式全局唯一，不可与 T1 撞名）。

use cucumber::{then, when};

use tauri_app_lib::physical_asset::{
    PhysicalAssetUpdateInput, PhysicalAssetValuationInput,
    update_physical_asset as update_physical_asset_domain,
    update_physical_asset_valuation as update_physical_asset_valuation_domain,
};

use crate::common::assert_last_error_contains;
use crate::world::LedgerWorld;

/// 哨兵值：「无」= 日期缺省（估值日期取今天）/ 金额缺省 / 币种缺省。
const NONE: &str = "无";

/// 更新估值入参组装：金额哨兵「无」→ None（触发必填报错路径）；
/// 币种 / 日期哨兵「无」→ None（币种触发必填报错，日期缺省 = 今天）。
fn build_valuation_input(amount: &str, currency: &str, date: &str) -> PhysicalAssetValuationInput {
    PhysicalAssetValuationInput {
        amount_cents: amount.parse::<i64>().ok(),
        currency_code: (!currency.eq_ignore_ascii_case(NONE)).then(|| currency.to_string()),
        valuation_date: (!date.eq_ignore_ascii_case(NONE)).then(|| date.into()),
    }
}

/// 编辑档案入参组装：购买价哨兵「无」→ None；购买价存在且币种给出时才携带
/// （触发成对校验路径），与建档步骤同形。
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

/// 定位最近创建资产的 id（T1 创建步骤写入；T2 写步骤前置条件）。
fn require_last_asset_id(world: &LedgerWorld) -> String {
    world
        .last_physical_asset_id
        .clone()
        .expect("更新 / 编辑前应先创建实物资产")
}

/// 更新估值（要求成功）：追加一条估值历史行；记录失效信号次数。
#[when(expr = "更新实物资产估值 金额 {string} 币种 {string} 估值日期 {string}")]
fn update_valuation(world: &mut LedgerWorld, amount: String, currency: String, date: String) {
    let id = require_last_asset_id(world);
    let input = build_valuation_input(&amount, &currency, &date);
    let mut signals = 0;
    match update_physical_asset_valuation_domain(&world_conn!(world), &id, input, &mut || {
        signals += 1
    }) {
        Ok(()) => world.physical_asset_signal_count = signals,
        Err(e) => panic!("更新实物资产估值应成功但失败: {e}"),
    }
}

/// 尝试更新估值并捕获错误（供「应返回错误」断言；失败不发信号）。
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
            world.physical_asset_signal_count = signals;
        }
    }
}

/// 编辑档案（要求成功）：只改名称与购买信息；记录失效信号次数。
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
        Ok(()) => world.physical_asset_signal_count = signals,
        Err(e) => panic!("编辑实物资产应成功但失败: {e}"),
    }
}

/// 尝试编辑档案并捕获错误（供「应返回错误」断言；失败不发信号）。
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
            world.physical_asset_signal_count = signals;
        }
    }
}

/// T2 操作失败断言（语义同 T1 创建错误断言，措辞面向 T2 入口）。
#[then(expr = "实物资产操作应返回错误 {string}")]
fn check_operation_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}

/// 编辑成功后版本递增断言（读模型 version 直读）。
#[then(expr = "第 {int} 件资产版本应为 {int}")]
fn assert_asset_version(world: &mut LedgerWorld, index: usize, expected: i64) {
    let asset = &world
        .physical_assets_list
        .as_ref()
        .expect("应先拉取列表快照")
        .assets[index - 1];
    assert_eq!(asset.version, expected, "资产版本不符");
}
