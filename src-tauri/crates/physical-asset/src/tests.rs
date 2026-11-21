use rusqlite::Connection;

use crate::{
    PhysicalAssetInput, PhysicalAssetValuationInput, create_physical_asset, get_physical_asset,
    list_physical_assets, update_physical_asset_valuation,
};

fn conn() -> Connection {
    tauri_app_lib::test_support::open()
}

fn valuation_input(amount_cents: i64, valuation_date: &str) -> PhysicalAssetValuationInput {
    PhysicalAssetValuationInput {
        amount_cents: Some(amount_cents),
        currency_code: Some("CNY".into()),
        valuation_date: Some(valuation_date.into()),
    }
}

#[test]
fn 同日连续追加估值_详情读回最后一条() {
    let conn = conn();
    let valuation_date = "2025-12-31";
    let id = create_physical_asset(
        &conn,
        PhysicalAssetInput {
            name: "整箱茅台".into(),
            purchase_date: None,
            purchase_price_cents: None,
            purchase_currency_code: None,
            initial_valuation_cents: Some(1),
            initial_valuation_currency_code: Some("CNY".into()),
            initial_valuation_date: Some(valuation_date.into()),
        },
        &mut || {},
    )
    .expect("创建实物资产失败");

    let mut last = 1;
    for amount in 2..=200 {
        update_physical_asset_valuation(
            &conn,
            &id,
            valuation_input(amount, valuation_date),
            &mut || {},
        )
        .expect("更新估值失败");
        last = amount;
    }

    let asset = get_physical_asset(&conn, &id).expect("读取详情失败");
    assert_eq!(
        asset.current_valuation_cents, last,
        "详情读回的当前估值应为最后追加的一条"
    );
}

use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};
use tauri_app_lib::test_support::{ScratchDir, open_file, seed_exchange_rate};

#[test]
fn physical_asset_list_total_and_rates_share_one_snapshot() {
    let dir = ScratchDir::new("physical-asset-read-snapshot");
    let conn = open_file(dir.path());
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    create_physical_asset(
        &conn,
        PhysicalAssetInput {
            name: "相机".into(),
            purchase_date: None,
            purchase_price_cents: None,
            purchase_currency_code: None,
            initial_valuation_cents: Some(10_000),
            initial_valuation_currency_code: Some("USD".into()),
            initial_valuation_date: None,
        },
        &mut || {},
    )
    .expect("创建实物资产失败");

    let before = list_physical_assets(&conn, None).unwrap();
    assert_eq!(
        before.holding_total_native_cents, 70_000,
        "种子合计应为 10000 分 × 汇率 7（否则口径断言空转）"
    );

    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM exchange_rates",
        &["UPDATE exchange_rates SET rate = rate * 2"],
    );
    let after = list_physical_assets(&conn, None).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中折算读取（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert_eq!(
        before.holding_total_native_cents, after.holding_total_native_cents,
        "家底合计必须与基线同时点（汇率落在语句间翻倍即漂移）"
    );
    assert_eq!(
        after.assets[0].current_valuation_native_cents,
        Some(70_000),
        "行折算值必须与合计同汇率快照"
    );
    assert_eq!(
        after.assets[0].native_currency, before.native_currency,
        "折算基准币种不应漂移"
    );
}

#[test]
fn physical_asset_detail_native_share_one_snapshot() {
    let dir = ScratchDir::new("physical-asset-detail-read-snapshot");
    let conn = open_file(dir.path());
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    let id = create_physical_asset(
        &conn,
        PhysicalAssetInput {
            name: "钢琴".into(),
            purchase_date: None,
            purchase_price_cents: None,
            purchase_currency_code: None,
            initial_valuation_cents: Some(20_000),
            initial_valuation_currency_code: Some("USD".into()),
            initial_valuation_date: None,
        },
        &mut || {},
    )
    .expect("创建实物资产失败");

    let before = get_physical_asset(&conn, &id).unwrap();
    assert_eq!(
        before.current_valuation_native_cents,
        Some(140_000),
        "种子折算值应为 20000 分 × 汇率 7（否则口径断言空转）"
    );

    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM exchange_rates",
        &["UPDATE exchange_rates SET rate = rate * 2"],
    );
    let after = get_physical_asset(&conn, &id).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中折算读取（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert_eq!(
        before.current_valuation_native_cents, after.current_valuation_native_cents,
        "详情折算值必须与基线同时点"
    );
    assert_eq!(
        before.native_currency, after.native_currency,
        "折算基准币种不应漂移"
    );
}
