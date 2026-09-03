//! 实物资产（PhysicalAsset）BDD 步骤（issue #466 / spec #465 / ADR-0064）：
//! 建档守卫、字段读回（含首条估值为当前值）、在持合计与失效信号计数。
//!
//! 经 `physical_asset` 域 API（`tauri_app_lib::physical_asset`）断言外部可观察
//! 行为：建档读回、校验错误信息、写后发失效信号（notify 注入，生产路径发
//! `ledger:changed`）、列表与在持合计。汇率 Given 复用 `scheduled_steps/occurrence`
//! 的已注册步骤（写 `exchange_rates` 当期表）。

use cucumber::{given, then, when};

use tauri_app_lib::physical_asset::{
    PhysicalAssetInput, create_physical_asset as create_physical_asset_domain,
    list_physical_assets as list_physical_assets_domain,
};

use crate::common::assert_last_error_contains;
use crate::world::LedgerWorld;

/// 哨兵值：「无」= 日期缺省（估值日期取今天）/ 金额缺省 / 币种缺省。
const NONE: &str = "无";

#[allow(clippy::too_many_arguments)] // cucumber step 签名由表达式参数决定，无法缩减
fn build_input(
    name: &str,
    purchase_date: &str,
    purchase_price: &str,
    purchase_currency: &str,
    valuation: &str,
    valuation_currency: &str,
    valuation_date: &str,
) -> PhysicalAssetInput {
    // 金额哨兵「无」→ None（缺失，触发必填报错路径）；存在时解析整数分。
    let price = purchase_price.parse::<i64>().ok();
    // 购买价存在且币种给出时才携带（触发成对校验路径），同保单保额先例。
    let purchase_currency_code = (price.is_some() && !purchase_currency.eq_ignore_ascii_case(NONE))
        .then(|| purchase_currency.to_string());
    PhysicalAssetInput {
        name: name.into(),
        purchase_date: (!purchase_date.eq_ignore_ascii_case(NONE)).then(|| purchase_date.into()),
        purchase_price_cents: price,
        purchase_currency_code,
        initial_valuation_cents: valuation.parse::<i64>().ok(),
        initial_valuation_currency_code: (!valuation_currency.eq_ignore_ascii_case(NONE))
            .then(|| valuation_currency.to_string()),
        initial_valuation_date: (!valuation_date.eq_ignore_ascii_case(NONE))
            .then(|| valuation_date.into()),
    }
}

/// 创建实物资产并要求成功；记录失效信号次数（写后发 `ledger:changed` 的 seam 断言）。
/// Given/When 双注册（先例 dashboard_steps 已买入）：其它域场景可作前置建档，
/// 也可在动作流中建档（#469 净资产第三腿场景复用）。
#[given(
    expr = "创建实物资产 名称 {string} 购买日期 {string} 购买价 {string} 币种 {string} 估值 {string} 估值币种 {string} 估值日期 {string}"
)]
#[when(
    expr = "创建实物资产 名称 {string} 购买日期 {string} 购买价 {string} 币种 {string} 估值 {string} 估值币种 {string} 估值日期 {string}"
)]
#[allow(clippy::too_many_arguments)] // cucumber step 签名由表达式参数决定，无法缩减
fn create_physical_asset(
    world: &mut LedgerWorld,
    name: String,
    purchase_date: String,
    purchase_price: String,
    purchase_currency: String,
    valuation: String,
    valuation_currency: String,
    valuation_date: String,
) {
    let input = build_input(
        &name,
        &purchase_date,
        &purchase_price,
        &purchase_currency,
        &valuation,
        &valuation_currency,
        &valuation_date,
    );
    let mut signals = 0;
    match create_physical_asset_domain(&world_conn!(world), input, &mut || signals += 1) {
        Ok(id) => {
            world.last_physical_asset_id = Some(id);
            world.physical_asset_signal_count = signals;
        }
        Err(e) => panic!("创建实物资产应成功但失败: {e}"),
    }
}

/// 尝试创建实物资产并捕获错误（供「应返回错误」断言；失败不发信号）。
#[when(
    expr = "尝试创建实物资产 名称 {string} 购买日期 {string} 购买价 {string} 币种 {string} 估值 {string} 估值币种 {string} 估值日期 {string}"
)]
#[allow(clippy::too_many_arguments)] // cucumber step 签名由表达式参数决定，无法缩减
fn try_create_physical_asset(
    world: &mut LedgerWorld,
    name: String,
    purchase_date: String,
    purchase_price: String,
    purchase_currency: String,
    valuation: String,
    valuation_currency: String,
    valuation_date: String,
) {
    let input = build_input(
        &name,
        &purchase_date,
        &purchase_price,
        &purchase_currency,
        &valuation,
        &valuation_currency,
        &valuation_date,
    );
    let mut signals = 0;
    match create_physical_asset_domain(&world_conn!(world), input, &mut || signals += 1) {
        Ok(_) => panic!("创建实物资产应失败但成功"),
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.physical_asset_signal_count = signals;
        }
    }
}

/// 读取最近创建资产的详情（详情读路径场景：与列表同一读口径）。
#[when(expr = "读取实物资产详情")]
fn get_physical_asset_detail(world: &mut LedgerWorld) {
    let id = world
        .last_physical_asset_id
        .clone()
        .expect("读取详情前应先创建实物资产");
    match tauri_app_lib::physical_asset::get_physical_asset(&world_conn!(world), &id) {
        Ok(asset) => world.physical_asset_detail = Some(asset),
        Err(e) => panic!("读取实物资产详情应成功但失败: {e}"),
    }
}

/// 拉取列表快照（默认口径 = 在持；合计口径恒为在持，与筛选无关）。
#[then(expr = "实物资产列表应包含 {int} 件资产")]
fn list_physical_assets(world: &mut LedgerWorld, expected: usize) {
    let list = list_physical_assets_domain(&world_conn!(world), None).expect("列表实物资产应成功");
    assert_eq!(
        list.assets.len(),
        expected,
        "实物资产列表件数不符: {:?}",
        list.assets
    );
    world.physical_assets_list = Some(list);
}

#[then(expr = "第 {int} 件资产名称应为 {string} 状态应为 {string}")]
fn assert_asset_name_status(world: &mut LedgerWorld, index: usize, name: String, status: String) {
    let asset = &world
        .physical_assets_list
        .as_ref()
        .expect("应先拉取列表快照")
        .assets[index - 1];
    assert_eq!(asset.name, name, "资产名称不符");
    assert_eq!(asset.status.as_str(), status, "资产状态不符");
}

#[then(expr = "第 {int} 件资产当前估值应为 {int} 币种 {string}")]
fn assert_asset_valuation(world: &mut LedgerWorld, index: usize, cents: i64, currency: String) {
    let asset = &world
        .physical_assets_list
        .as_ref()
        .expect("应先拉取列表快照")
        .assets[index - 1];
    assert_eq!(asset.current_valuation_cents, cents, "当前估值不符");
    assert_eq!(
        asset.current_valuation_currency_code, currency,
        "当前估值币种不符"
    );
}

/// 估值日期缺省 = 建档当天的本地今天（域内取当前日期，先例物品使用成本）。
#[then(expr = "第 {int} 件资产估值日期应为今天")]
fn assert_asset_valuation_today(world: &mut LedgerWorld, index: usize) {
    let today = chrono::Local::now().date_naive().to_string();
    let asset = &world
        .physical_assets_list
        .as_ref()
        .expect("应先拉取列表快照")
        .assets[index - 1];
    assert_eq!(asset.current_valuation_date, today, "估值日期应为今天");
}

#[then(expr = "第 {int} 件资产当前估值日期应为 {string}")]
fn assert_asset_valuation_date(world: &mut LedgerWorld, index: usize, date: String) {
    let asset = &world
        .physical_assets_list
        .as_ref()
        .expect("应先拉取列表快照")
        .assets[index - 1];
    assert_eq!(asset.current_valuation_date, date, "估值日期不符");
}

#[then(expr = "第 {int} 件资产购买信息应为空")]
fn assert_asset_purchase_empty(world: &mut LedgerWorld, index: usize) {
    let asset = &world
        .physical_assets_list
        .as_ref()
        .expect("应先拉取列表快照")
        .assets[index - 1];
    assert!(asset.purchase_date.is_none(), "购买日期应为空");
    assert!(asset.purchase_price_cents.is_none(), "购买价应为空");
    assert!(
        asset.purchase_currency_code.is_none(),
        "购买币种应为空（与购买价成对缺省）"
    );
}

#[then(expr = "第 {int} 件资产购买日期应为 {string} 购买价应为 {int} 币种 {string}")]
fn assert_asset_purchase(
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
    assert_eq!(asset.purchase_date.as_deref(), Some(date.as_str()));
    assert_eq!(asset.purchase_price_cents, Some(cents));
    assert_eq!(
        asset.purchase_currency_code.as_deref(),
        Some(currency.as_str())
    );
}

#[then(expr = "第 {int} 件资产当前估值折本位币应为 {int} 币种 {string}")]
fn assert_asset_valuation_native(
    world: &mut LedgerWorld,
    index: usize,
    cents: i64,
    currency: String,
) {
    let asset = &world
        .physical_assets_list
        .as_ref()
        .expect("应先拉取列表快照")
        .assets[index - 1];
    assert_eq!(
        asset.current_valuation_native_cents,
        Some(cents),
        "当前估值折本位币不符"
    );
    assert_eq!(asset.native_currency, currency, "本位币代码不符");
}

#[then(expr = "在持估值合计应为 {int} 币种 {string}")]
fn assert_holding_total(world: &mut LedgerWorld, cents: i64, currency: String) {
    let list = world
        .physical_assets_list
        .as_ref()
        .expect("应先拉取列表快照");
    assert_eq!(list.holding_total_native_cents, cents, "在持估值合计不符");
    assert_eq!(list.native_currency, currency, "合计本位币代码不符");
}

/// 唯一 ID + 审计字段（UUID v7 + 软删标志复位 + 版本起点）。
#[then(expr = "第 {int} 件资产应有唯一 ID 与审计字段")]
fn assert_asset_audit(world: &mut LedgerWorld, index: usize) {
    let asset = &world
        .physical_assets_list
        .as_ref()
        .expect("应先拉取列表快照")
        .assets[index - 1];
    assert!(!asset.id.is_empty(), "资产应有唯一 ID");
    assert_eq!(asset.version, 1, "新建资产版本应为 1");
    assert!(!asset.is_deleted, "新建资产不应是软删态");
    assert!(!asset.created_at.is_empty(), "应有创建时间");
    assert_eq!(asset.created_at, asset.updated_at, "新建资产时间戳应一致");
    assert!(!asset.device_id.is_empty(), "应有设备标识");
}

#[then(expr = "实物资产写入后应发出 {int} 次失效信号")]
fn check_signals(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(
        world.physical_asset_signal_count, expected,
        "失效信号次数不符"
    );
}

#[then(expr = "实物资产未发出失效信号")]
fn check_no_signal(world: &mut LedgerWorld) {
    assert_eq!(
        world.physical_asset_signal_count, 0,
        "失败路径不应发出失效信号"
    );
}

#[then(expr = "实物资产创建应返回错误 {string}")]
fn check_create_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}

#[then(expr = "实物资产详情当前估值应为 {int} 币种 {string} 折本位币应为 {int}")]
fn assert_detail_valuation(
    world: &mut LedgerWorld,
    cents: i64,
    currency: String,
    native_cents: i64,
) {
    let asset = world.physical_asset_detail.as_ref().expect("应先读取详情");
    assert_eq!(asset.current_valuation_cents, cents, "详情当前估值不符");
    assert_eq!(
        asset.current_valuation_currency_code, currency,
        "详情当前估值币种不符"
    );
    assert_eq!(
        asset.current_valuation_native_cents,
        Some(native_cents),
        "详情当前估值折本位币不符"
    );
    // 折算基准恒为全局默认币种（Amount 接缝）。
    assert_eq!(asset.native_currency, "CNY", "详情本位币代码不符");
}
