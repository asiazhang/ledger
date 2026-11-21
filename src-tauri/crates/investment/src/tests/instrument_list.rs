use crate::InstrumentType;
use ledger_transaction::create_transaction_internal;
use rusqlite::{Connection, params};

use super::common::*;
use tauri_app_lib::test_support::{
    open, seed_account, seed_fx_history_weeks, seed_instrument, seed_market_price,
};

#[test]
fn list_instruments_pagination_and_search() {
    let conn = open();
    for i in 0..5 {
        insert_instrument_with_market(
            &conn,
            &format!("inst-list-{i}"),
            &format!("SYM{i}"),
            &format!("名称{i}"),
            "USD",
            if i % 2 == 0 { "sh" } else { "sz" },
            "stock",
        );
    }

    let filter = InstrumentListFilter::default();
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 5);
    assert_eq!(result.items.len(), 5);
    assert_eq!(result.items[0].symbol, "SYM0");

    let filter = InstrumentListFilter {
        search: None,
        market: None,
        kind: None,
        only_invested: None,
        page: None,
        page_size: Some(2),
        anchor_id: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 5);
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].symbol, "SYM0");
    assert_eq!(result.items[1].symbol, "SYM1");

    let filter = InstrumentListFilter {
        search: None,
        market: None,
        kind: None,
        only_invested: None,
        page: Some(2),
        page_size: Some(2),
        anchor_id: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].symbol, "SYM2");

    let filter = InstrumentListFilter {
        search: Some("sym1".into()),
        market: None,
        kind: None,
        only_invested: None,
        page: None,
        page_size: None,
        anchor_id: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "SYM1");

    let filter = InstrumentListFilter {
        search: Some("名称3".into()),
        market: None,
        kind: None,
        only_invested: None,
        page: None,
        page_size: None,
        anchor_id: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "SYM3");

    let filter = InstrumentListFilter {
        search: None,
        market: Some("sh".into()),
        kind: None,
        only_invested: None,
        page: None,
        page_size: None,
        anchor_id: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 3);
    assert!(result.items.iter().all(|i| i.market == "sh"));

    let filter = InstrumentListFilter {
        search: Some("SYM".into()),
        market: Some("sz".into()),
        kind: None,
        only_invested: None,
        page: Some(2),
        page_size: Some(1),
        anchor_id: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 2);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].symbol, "SYM3");
}

#[test]
fn list_instruments_search_pinyin_semantics() {
    let conn = open();
    insert_instrument_with_market(&conn, "inst-zs", "600519", "招商银行", "CNY", "sh", "stock");
    insert_instrument_with_market(&conn, "inst-wk", "000002", "万科物业", "CNY", "sz", "stock");
    insert_instrument_with_market(&conn, "inst-abc", "ABCH", "ABC银行", "CNY", "sh", "stock");

    let result = search_all(&conn, "zsyh");
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "600519");

    let result = search_all(&conn, "wy");
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "000002");

    assert_eq!(search_all(&conn, "ZSYH").total, 1);
    assert_eq!(search_all(&conn, "万科").total, 1);

    let result = search_all(&conn, "ac");
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "ABCH");

    let result = search_all(&conn, "600 zs");
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "600519");

    assert_eq!(search_all(&conn, "bc银").total, 1);

    assert_eq!(search_all(&conn, "招zs").total, 0);

    assert_eq!(search_all(&conn, "zsyh wy").total, 0);
}

fn search_all(conn: &Connection, search: &str) -> InstrumentListResult {
    super::crud::list_instruments(
        conn,
        &InstrumentListFilter {
            search: Some(search.into()),
            market: None,
            kind: None,
            only_invested: None,
            page: None,
            page_size: None,
            anchor_id: None,
        },
    )
    .unwrap()
}

#[test]
fn list_instruments_invested_flag() {
    let conn = open();
    seed_account(&conn, "acc-inv", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    insert_instrument_with_market(&conn, "inst-held", "HELD", "持仓标的", "USD", "sh", "stock");
    insert_instrument_with_market(
        &conn,
        "inst-closed",
        "CLOSED",
        "已清仓标的",
        "USD",
        "sz",
        "stock",
    );
    insert_instrument_with_market(
        &conn,
        "inst-never",
        "NEVER",
        "未投资标的",
        "USD",
        "hk",
        "stock",
    );

    create_transaction_internal(
        &conn,
        make_buy_input("acc-inv", "inst-held", 10.0, 10000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_buy_input("acc-inv", "inst-closed", 10.0, 10000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-inv", "inst-closed", 10.0, 12000, 0),
    )
    .unwrap();

    let filter = InstrumentListFilter::default();
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 3);
    let invested_by_symbol: Vec<(&str, bool)> = result
        .items
        .iter()
        .map(|i| (i.symbol.as_str(), i.invested))
        .collect();
    assert_eq!(
        invested_by_symbol,
        vec![("CLOSED", false), ("HELD", true), ("NEVER", false)],
        "持仓中为 true，已清仓/未投资为 false"
    );
}

#[test]
fn list_instruments_only_invested_filter() {
    let conn = open();
    seed_account(&conn, "acc-inv", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    insert_instrument_with_market(&conn, "inst-held", "HELD", "持仓标的", "USD", "sh", "stock");
    insert_instrument_with_market(
        &conn,
        "inst-closed",
        "CLOSED",
        "已清仓标的",
        "USD",
        "sz",
        "stock",
    );
    insert_instrument_with_market(
        &conn,
        "inst-never",
        "NEVER",
        "未投资标的",
        "USD",
        "hk",
        "stock",
    );

    create_transaction_internal(
        &conn,
        make_buy_input("acc-inv", "inst-held", 10.0, 10000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_buy_input("acc-inv", "inst-closed", 10.0, 10000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-inv", "inst-closed", 10.0, 12000, 0),
    )
    .unwrap();

    let filter = InstrumentListFilter {
        search: None,
        market: None,
        kind: None,
        only_invested: Some(true),
        page: None,
        page_size: None,
        anchor_id: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].symbol, "HELD");
    assert!(result.items[0].invested);

    let filter = InstrumentListFilter {
        search: Some("CLOSED".into()),
        market: None,
        kind: None,
        only_invested: Some(true),
        page: None,
        page_size: None,
        anchor_id: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 0);
    assert!(result.items.is_empty());

    let filter = InstrumentListFilter {
        search: None,
        market: Some("sh".into()),
        kind: None,
        only_invested: Some(true),
        page: None,
        page_size: None,
        anchor_id: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "HELD");

    let filter = InstrumentListFilter {
        search: None,
        market: None,
        kind: None,
        only_invested: Some(true),
        page: Some(2),
        page_size: Some(1),
        anchor_id: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert!(result.items.is_empty());

    let filter = InstrumentListFilter {
        search: None,
        market: None,
        kind: None,
        only_invested: Some(false),
        page: None,
        page_size: None,
        anchor_id: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 3);
    let filter = InstrumentListFilter::default();
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 3);
}

#[test]
fn list_instruments_invested_excludes_soft_deleted_accounts() {
    let conn = open();
    seed_account(&conn, "acc-del", "已删账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    insert_instrument_with_market(
        &conn,
        "inst-del",
        "DELHOLD",
        "删除账户持仓",
        "USD",
        "sh",
        "stock",
    );

    create_transaction_internal(&conn, make_buy_input("acc-del", "inst-del", 10.0, 10000, 0))
        .unwrap();
    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id='acc-del'", [])
        .unwrap();

    let filter = InstrumentListFilter::default();
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    let item = result.items.iter().find(|i| i.symbol == "DELHOLD").unwrap();
    assert!(!item.invested, "软删除账户的持仓不应计入 invested");

    let filter = InstrumentListFilter {
        search: None,
        market: None,
        kind: None,
        only_invested: Some(true),
        page: None,
        page_size: None,
        anchor_id: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 0);
}

#[test]
fn list_instruments_kind_filter_disambiguates_same_symbol() {
    let conn = open();
    insert_instrument_with_market(
        &conn,
        "inst-fund",
        "000001",
        "华夏成长混合",
        "CNY",
        "sz",
        "fund",
    );
    insert_instrument_with_market(
        &conn,
        "inst-stock",
        "000001",
        "平安银行",
        "CNY",
        "sz",
        "stock",
    );

    let result = super::crud::list_instruments(&conn, &InstrumentListFilter::default()).unwrap();
    assert_eq!(result.total, 2);

    for (kind, expected_name) in [
        (InstrumentType::Fund, "华夏成长混合"),
        (InstrumentType::Stock, "平安银行"),
    ] {
        let filter = InstrumentListFilter {
            kind: Some(kind),
            ..Default::default()
        };
        let result = super::crud::list_instruments(&conn, &filter).unwrap();
        assert_eq!(result.total, 1, "{kind} 应只命中一行");
        assert_eq!(result.items[0].name.as_deref(), Some(expected_name));
        assert_eq!(result.items[0].kind, kind);
    }

    let filter = InstrumentListFilter {
        search: Some("000001".into()),
        kind: Some(InstrumentType::Fund),
        ..Default::default()
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].kind, InstrumentType::Fund);

    let filter = InstrumentListFilter {
        kind: Some(InstrumentType::Bond),
        ..Default::default()
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 0);
}

#[test]
fn list_instruments_empty_initially() {
    let conn = open();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn create_instrument_inserts_and_returns_id() {
    let conn = open();
    let id = seed_instrument(
        &conn,
        &ledger_infra::db::new_uuid(),
        "NVDA",
        "NVIDIA Corporation",
        "USD",
        "unknown",
    );
    let (symbol, name, ccy): (String, Option<String>, String) = conn
        .query_row(
            "SELECT symbol, name, currency_code FROM instruments WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(symbol, "NVDA");
    assert_eq!(name.as_deref(), Some("NVIDIA Corporation"));
    assert_eq!(ccy, "USD");
}

#[test]
fn create_instrument_is_idempotent() {
    let conn = open();
    let id1 = ledger_infra::db::new_uuid();
    let id2 = ledger_infra::db::new_uuid();
    seed_instrument(&conn, &id1, "AAPL", "Apple Inc.", "USD", "unknown");
    let result = try_insert_instrument_with_market(
        &conn,
        &id2,
        "AAPL",
        "Apple Again",
        "USD",
        "unknown",
        "stock",
    );
    assert!(result.is_err());
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM instruments WHERE symbol='AAPL' AND instrument_type='stock'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn list_holdings_returns_after_buy_and_market_price() {
    let conn = open();
    seed_account(&conn, "acc-hold", "投资账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-hold", "GOOGL", "Alphabet", "USD", "unknown");

    let buy_input = make_buy_input("acc-hold", "inst-hold", 10.0, 1_500_000, 1000);
    create_transaction_internal(&conn, buy_input).unwrap();

    seed_market_price(&conn, "inst-hold", 1_600_000, "USD");

    let (qty, cost_basis, market_value, unrealized_pnl): (f64, i64, i64, i64) = conn
        .query_row(
            "SELECT quantity, cost_basis_cents, market_value_cents, unrealized_pnl_cents \
             FROM v_holdings WHERE instrument_id=?1",
            params!["inst-hold"],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert!((qty - 10.0).abs() < 0.0001);
    assert_eq!(cost_basis, 151000);
    assert_eq!(market_value, 160000);
    assert_eq!(unrealized_pnl, 9000);
}

fn seed_symbols(conn: &Connection, n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            insert_instrument_with_market(
                conn,
                &format!("inst-anchor-{i}"),
                &format!("SYM{i}"),
                &format!("名称{i}"),
                "CNY",
                "sh",
                "stock",
            );
            format!("inst-anchor-{i}")
        })
        .collect()
}

#[test]
fn anchor_id_locates_page_and_returns_that_page() {
    let conn = open();
    let ids = seed_symbols(&conn, 5);

    let result = super::crud::list_instruments(
        &conn,
        &InstrumentListFilter {
            page_size: Some(2),
            anchor_id: Some(ids[3].clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.anchor_page, Some(2));
    assert_eq!(result.total, 5);
    let symbols: Vec<&str> = result.items.iter().map(|i| i.symbol.as_str()).collect();
    assert_eq!(symbols, ["SYM2", "SYM3"]);

    let result = super::crud::list_instruments(
        &conn,
        &InstrumentListFilter {
            page_size: Some(2),
            anchor_id: Some(ids[4].clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.anchor_page, Some(3));
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].symbol, "SYM4");
}

#[test]
fn anchor_id_respects_active_filter_when_locating() {
    let conn = open();
    let ids = seed_symbols(&conn, 5);
    let result = super::crud::list_instruments(
        &conn,
        &InstrumentListFilter {
            search: Some("名称4".into()),
            page_size: Some(2),
            anchor_id: Some(ids[4].clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.anchor_page, Some(1));
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "SYM4");
}

#[test]
fn anchor_id_not_in_result_set_falls_back_to_plain_page() {
    let conn = open();
    let ids = seed_symbols(&conn, 5);

    let result = super::crud::list_instruments(
        &conn,
        &InstrumentListFilter {
            page: Some(2),
            page_size: Some(2),
            anchor_id: Some("inst-missing".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.anchor_page, None);
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].symbol, "SYM2");

    let result = super::crud::list_instruments(
        &conn,
        &InstrumentListFilter {
            only_invested: Some(true),
            anchor_id: Some(ids[0].clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.anchor_page, None);
    assert_eq!(result.total, 0);
    assert!(result.items.is_empty());
}
