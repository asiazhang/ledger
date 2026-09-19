//! `physical-asset` 域单元测试：当前估值「同日按插入序」读口径的回归
//! （issue #1489）。
//!
//! 本单测面向用户可观察结果——详情读回的当前估值，复现「更新估值后详情仍读旧值」
//! 的症状；根因与确定性守门在基础设施侧（`ledger_infra` 的 `new_uuid` 进程内单调
//! 不变量及其单测）。外部可观察行为的跨模块验收在 BDD
//! `physical_asset_updates.feature`。

use rusqlite::Connection;

use crate::{
    PhysicalAssetInput, PhysicalAssetValuationInput, create_physical_asset, get_physical_asset,
    update_physical_asset_valuation,
};

fn conn() -> Connection {
    // 建库经统一测试工厂（ADR-0084），dev-dependency 环消费根包器具（先例 policy）。
    tauri_app_lib::test_support::open()
}

fn valuation_input(amount_cents: i64, valuation_date: &str) -> PhysicalAssetValuationInput {
    PhysicalAssetValuationInput {
        amount_cents: Some(amount_cents),
        currency_code: Some("CNY".into()),
        valuation_date: Some(valuation_date.into()),
    }
}

/// 同一天连续追加估值后，详情读回的当前估值必须是最后追加的一条。
///
/// 同日估值按 `valuation_date DESC, id DESC` 取最新，插入序依赖 `new_uuid` 的
/// 进程内单调性（issue #1489）。连续追加在毫秒尺度上密集发生，旧 `NoContext`
/// 实现下详情会读到非最后一条；确定性「删修复即变红」守门见 `ledger_infra` 的
/// 单调单测。
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
