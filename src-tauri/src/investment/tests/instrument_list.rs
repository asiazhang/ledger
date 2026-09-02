//! 标的列表与 CRUD 读路径测试：分页 / 搜索（拼音语义）/ invested 派生与过滤 /
//! 软删除账户排除 / 建标的幂等 / 持仓视图（issue #257 纯移动归组）。

use crate::commands::transactions::create_transaction_internal;
use crate::models::InstrumentType;
use rusqlite::{Connection, params};

use super::common::*;

#[test]
fn list_instruments_pagination_and_search() {
    let conn = setup_db();
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

    // 默认第一页（page_size=50），返回全量
    let filter = InstrumentListFilter::default();
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 5);
    assert_eq!(result.items.len(), 5);
    assert_eq!(result.items[0].symbol, "SYM0");

    // 分页：每页 2 条，第 1 页
    let filter = InstrumentListFilter {
        search: None,
        market: None,
        kind: None,
        only_invested: None,
        page: None,
        page_size: Some(2),
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 5);
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].symbol, "SYM0");
    assert_eq!(result.items[1].symbol, "SYM1");

    // 分页：第 2 页
    let filter = InstrumentListFilter {
        search: None,
        market: None,
        kind: None,
        only_invested: None,
        page: Some(2),
        page_size: Some(2),
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].symbol, "SYM2");

    // 搜索：代码大小写不敏感
    let filter = InstrumentListFilter {
        search: Some("sym1".into()),
        market: None,
        kind: None,
        only_invested: None,
        page: None,
        page_size: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "SYM1");

    // 搜索：名称
    let filter = InstrumentListFilter {
        search: Some("名称3".into()),
        market: None,
        kind: None,
        only_invested: None,
        page: None,
        page_size: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "SYM3");

    // 市场筛选
    let filter = InstrumentListFilter {
        search: None,
        market: Some("sh".into()),
        kind: None,
        only_invested: None,
        page: None,
        page_size: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 3);
    assert!(result.items.iter().all(|i| i.market == "sh"));

    // 搜索 + 市场组合
    let filter = InstrumentListFilter {
        search: Some("SYM".into()),
        market: Some("sz".into()),
        kind: None,
        only_invested: None,
        page: Some(2),
        page_size: Some(1),
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 2);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].symbol, "SYM3");
}

/// 标的搜索统一模糊语义（issue #199，ADR-0027）：子串 ∨ 拼音首字母子序列、
/// 词条 AND、大小写不敏感，判定目标为下拉 label 等价文本「代码 · 名称」。
#[test]
fn list_instruments_search_pinyin_semantics() {
    let conn = setup_db();
    insert_instrument_with_market(&conn, "inst-zs", "600519", "招商银行", "CNY", "sh", "stock");
    insert_instrument_with_market(&conn, "inst-wk", "000002", "万科物业", "CNY", "sz", "stock");
    insert_instrument_with_market(&conn, "inst-abc", "ABCH", "ABC银行", "CNY", "sh", "stock");

    // 拼音首字母整串命中（多音字修正：银行 → yh）
    let result = search_all(&conn, "zsyh");
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "600519");

    // 首字母子序列跳字命中（不要求连续）：wy → 万科物业（wkwy）
    let result = search_all(&conn, "wy");
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "000002");

    // 大小写不敏感（原文与拼音两路径均不区分大小写）
    assert_eq!(search_all(&conn, "ZSYH").total, 1);
    assert_eq!(search_all(&conn, "万科").total, 1);

    // ASCII 首字母串的子序列命中：ac ⊂ abcyh（非子串路径）
    let result = search_all(&conn, "ac");
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "ABCH");

    // 多词条 AND：词条分别命中代码与名称
    let result = search_all(&conn, "600 zs");
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "600519");

    // 混合词条对混合内容（「ABC银行」）：原文子串路径命中
    assert_eq!(search_all(&conn, "bc银").total, 1);

    // 含汉字的词条对纯中文内容的 ASCII 首字母串必败、原文子串也不含 → 不命中
    assert_eq!(search_all(&conn, "招zs").total, 0);

    // 无命中
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
        },
    )
    .unwrap()
}

/// invested 派生字段：持仓中为 true，未投资 / 已清仓为 false（issue #102）。
#[test]
fn list_instruments_invested_flag() {
    let conn = setup_db();
    insert_account(&conn, "acc-inv", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    // 持仓中：买入 10 股，未卖出
    insert_instrument_with_market(&conn, "inst-held", "HELD", "持仓标的", "USD", "sh", "stock");
    // 已清仓：买入 10 股后全部卖出
    insert_instrument_with_market(
        &conn,
        "inst-closed",
        "CLOSED",
        "已清仓标的",
        "USD",
        "sz",
        "stock",
    );
    // 未投资：从未交易
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

/// only_invested 过滤：与搜索、市场过滤、分页组合正确（issue #102）。
#[test]
fn list_instruments_only_invested_filter() {
    let conn = setup_db();
    insert_account(&conn, "acc-inv", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
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

    // 只看持仓：仅返回持仓中标的
    let filter = InstrumentListFilter {
        search: None,
        market: None,
        kind: None,
        only_invested: Some(true),
        page: None,
        page_size: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].symbol, "HELD");
    assert!(result.items[0].invested);

    // 与搜索组合：命中已清仓标的时结果为空
    let filter = InstrumentListFilter {
        search: Some("CLOSED".into()),
        market: None,
        kind: None,
        only_invested: Some(true),
        page: None,
        page_size: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 0);
    assert!(result.items.is_empty());

    // 与市场过滤组合：只保留市场命中且持仓中的标的
    let filter = InstrumentListFilter {
        search: None,
        market: Some("sh".into()),
        kind: None,
        only_invested: Some(true),
        page: None,
        page_size: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "HELD");

    // 与分页组合：total 按过滤后全量计数，越界页返回空
    let filter = InstrumentListFilter {
        search: None,
        market: None,
        kind: None,
        only_invested: Some(true),
        page: Some(2),
        page_size: Some(1),
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert!(result.items.is_empty());

    // only_invested 为 false 或缺省时行为一致：不过滤
    let filter = InstrumentListFilter {
        search: None,
        market: None,
        kind: None,
        only_invested: Some(false),
        page: None,
        page_size: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 3);
    let filter = InstrumentListFilter::default();
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 3);
}

/// 软删除账户的持仓批次不计入 invested（口径与 v_holdings 一致，issue #102）。
#[test]
fn list_instruments_invested_excludes_soft_deleted_accounts() {
    let conn = setup_db();
    insert_account(&conn, "acc-del", "已删账户", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
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
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 0);
}

/// 标的类型过滤（issue #294）：同码异类型消歧（如基金 000001 vs 股票 000001）。
#[test]
fn list_instruments_kind_filter_disambiguates_same_symbol() {
    let conn = setup_db();
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

    // 不过滤：同码异类型两行并返
    let result = super::crud::list_instruments(&conn, &InstrumentListFilter::default()).unwrap();
    assert_eq!(result.total, 2);

    // kind 过滤消歧：fund / stock 各一行
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

    // 与搜索组合：词条命中 label 后再按类型收敛
    let filter = InstrumentListFilter {
        search: Some("000001".into()),
        kind: Some(InstrumentType::Fund),
        ..Default::default()
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].kind, InstrumentType::Fund);

    // 无该类型的同码标的：不命中
    let filter = InstrumentListFilter {
        kind: Some(InstrumentType::Bond),
        ..Default::default()
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 0);
}

#[test]
fn list_instruments_empty_initially() {
    let conn = setup_db();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn create_instrument_inserts_and_returns_id() {
    let conn = setup_db();
    let id = crate::db::new_uuid();
    let now = crate::db::now_iso();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'stock',?3,?4,'unknown',?5,?6,?7,?8)",
        params![id, "NVDA", "NVIDIA Corporation", "USD", now, now, 1, "test"],
    ).unwrap();
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
    let conn = setup_db();
    let id1 = crate::db::new_uuid();
    let id2 = crate::db::new_uuid();
    let now = crate::db::now_iso();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,'AAPL','stock',?2,'USD','unknown',?3,?4,?5,?6)",
        params![id1, "Apple Inc.", now, now, 1, "test"],
    ).unwrap();
    let result = conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,'AAPL','stock',?2,'USD','unknown',?3,?4,?5,?6)",
        params![id2, "Apple Again", now, now, 1, "test"],
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
    let conn = setup_db();
    insert_account(&conn, "acc-hold", "投资账户", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-hold", "GOOGL", "Alphabet", "USD");

    let buy_input = make_buy_input("acc-hold", "inst-hold", 10.0, 1_500_000, 1000);
    create_transaction_internal(&conn, buy_input).unwrap();

    let now = crate::db::now_iso();
    let price_id = crate::db::new_uuid();
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,1600000,'USD',?3,NULL,?4,?5,?6,?7)",
        params![price_id, "inst-hold", now, now, now, 1, "test"],
    ).unwrap();

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
