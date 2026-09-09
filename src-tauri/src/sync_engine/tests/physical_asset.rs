//! 实物资产域的全域 op 产出与重放收敛（issue #860）：建档（首条估值随行）、
//! 编辑、估值追加（只追加不改写、无 LWW——并发估值全部存活）、处置与软删。

use super::super::{OpOutcome, read_ops};
use super::common::{seed_device, wire_in, wire_out};
use crate::physical_asset::{
    PhysicalAssetDisposeInput, PhysicalAssetInput, PhysicalAssetUpdateInput,
    PhysicalAssetValuationInput, create_physical_asset, delete_physical_asset,
    dispose_physical_asset, update_physical_asset, update_physical_asset_valuation,
};
use crate::test_support;

fn asset_input(name: &str) -> PhysicalAssetInput {
    PhysicalAssetInput {
        name: name.into(),
        purchase_date: Some("2025-06-01".into()),
        purchase_price_cents: Some(3_000_000),
        purchase_currency_code: Some("CNY".into()),
        initial_valuation_cents: Some(1_000_000),
        initial_valuation_currency_code: Some("CNY".into()),
        initial_valuation_date: Some("2026-01-01".into()),
    }
}

#[test]
fn physical_asset_lifecycle_ops_replay_and_converge() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let id = create_physical_asset(&conn_a, asset_input("房子"), &mut || {}).unwrap();
    update_physical_asset(
        &conn_a,
        &id,
        PhysicalAssetUpdateInput {
            name: "公寓".into(),
            purchase_date: None,
            purchase_price_cents: None,
            purchase_currency_code: None,
        },
        &mut || {},
    )
    .unwrap();
    update_physical_asset_valuation(
        &conn_a,
        &id,
        PhysicalAssetValuationInput {
            amount_cents: Some(1_500_000),
            currency_code: Some("CNY".into()),
            valuation_date: Some("2026-03-01".into()),
        },
        &mut || {},
    )
    .unwrap();
    dispose_physical_asset(
        &conn_a,
        &id,
        PhysicalAssetDisposeInput {
            disposal_date: Some("2026-04-01".into()),
            disposal_price_cents: Some(1_600_000),
            disposal_currency_code: Some("CNY".into()),
        },
        &mut || {},
    )
    .unwrap();

    let ops = read_ops(&conn_a).unwrap();
    assert_eq!(ops.len(), 4, "建档/编辑/估值/处置各一条：{ops:?}");
    assert!(ops.iter().all(|op| op.command.entity() == "physical_asset"));

    wire_in(&conn_b, &wire_out(&conn_a));

    let valuations: i64 = conn_b
        .query_row(
            "SELECT COUNT(*) FROM physical_asset_valuations WHERE asset_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(valuations, 2, "两条估值历史行全部存活（首条 + 追加）");
    let row = |conn: &rusqlite::Connection| -> (String, String, Option<i64>, bool) {
        conn.query_row(
            "SELECT a.name, a.status, v.amount_cents, a.is_deleted \
             FROM physical_assets a \
             JOIN physical_asset_valuations v ON v.asset_id = a.id \
               AND v.id = (SELECT v2.id FROM physical_asset_valuations v2 \
                           WHERE v2.asset_id = a.id \
                           ORDER BY v2.valuation_date DESC, v2.id DESC LIMIT 1) \
             WHERE a.id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0)),
        )
        .unwrap()
    };
    assert_eq!(row(&conn_a), row(&conn_b), "重放后资产与当前估值一致");
    assert_eq!(row(&conn_b).1, "disposed", "处置状态收敛");
    assert_eq!(row(&conn_b).2, Some(1_500_000), "当前估值 = 最新一条");

    delete_physical_asset(&conn_a, &id, &mut || {}).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    let deleted: i64 = conn_b
        .query_row(
            "SELECT is_deleted FROM physical_assets WHERE id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(deleted, 1, "软删收敛（估值历史保留）");
}

#[test]
fn concurrent_valuations_all_survive_without_lww() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let id = create_physical_asset(&conn_a, asset_input("车子"), &mut || {}).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));

    // 两端并发各追加一次估值（不同估值日期）：估值是只追加历史行，无实体指向、
    // 不参与同实体 LWW——两条都必须存活。
    update_physical_asset_valuation(
        &conn_a,
        &id,
        PhysicalAssetValuationInput {
            amount_cents: Some(900_000),
            currency_code: Some("CNY".into()),
            valuation_date: Some("2026-03-01".into()),
        },
        &mut || {},
    )
    .unwrap();
    update_physical_asset_valuation(
        &conn_b,
        &id,
        PhysicalAssetValuationInput {
            amount_cents: Some(1_100_000),
            currency_code: Some("CNY".into()),
            valuation_date: Some("2026-03-05".into()),
        },
        &mut || {},
    )
    .unwrap();

    let reports_on_b = wire_in(&conn_b, &wire_out(&conn_a));
    wire_in(&conn_a, &wire_out(&conn_b));
    assert!(
        reports_on_b
            .iter()
            .all(|r| r.outcome != OpOutcome::Superseded),
        "估值追加不互相压制：{reports_on_b:?}"
    );

    let count = |conn: &rusqlite::Connection| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM physical_asset_valuations WHERE asset_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(count(&conn_a), 3, "首条 + 两端并发估值全部存活");
    assert_eq!(count(&conn_a), count(&conn_b));
    let current: i64 = conn_b
        .query_row(
            "SELECT amount_cents FROM physical_asset_valuations \
             WHERE asset_id=?1 ORDER BY valuation_date DESC, id DESC LIMIT 1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(current, 1_100_000, "当前估值由读口径裁决为最新一条");
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}
