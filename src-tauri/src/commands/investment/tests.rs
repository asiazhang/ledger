use super::*;
use rusqlite::{Connection, params};

use crate::commands::transactions::create_transaction_internal;
use crate::models::{PnlFilter, TransactionInput};
use crate::transaction::amount::TransactionKind;

fn setup_db() -> Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

fn insert_account(conn: &Connection, id: &str, name: &str, kind: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![id, name, kind, currency],
    ).unwrap();
}

fn insert_instrument(conn: &Connection, id: &str, symbol: &str, name: &str, currency: &str) {
    conn.execute(
         "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
          VALUES (?1,?2,'stock',?3,?4,'unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, symbol, name, currency],
    ).unwrap();
}

/// buy/sell 本位币折算走 Amount 接缝（issue #70）：测试库补 1:1 汇率，
/// 非默认币种（USD）账户的交易折算不报缺汇率。
fn insert_rate_1_1(conn: &Connection, base: &str) {
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-1-1',?1,'CNY',1.0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![base],
    )
    .unwrap();
}

/// 补一条指定汇率（供非 1:1 折算断言用，如 7.2）。
fn insert_rate(conn: &Connection, base: &str, quote: &str, rate: f64) {
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-rate',?1,?2,?3,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![base, quote, rate],
    )
    .unwrap();
}

fn insert_instrument_with_market(
    conn: &Connection,
    id: &str,
    symbol: &str,
    name: &str,
    currency: &str,
    market: &str,
) {
    conn.execute(
         "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
          VALUES (?1,?2,'stock',?3,?4,?5,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, symbol, name, currency, market],
    ).unwrap();
}

fn make_buy_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(fee),
        idempotency_key: None,
    }
}

fn make_sell_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        kind: TransactionKind::Sell,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-20".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(fee),
        idempotency_key: None,
    }
}

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
    insert_instrument_with_market(&conn, "inst-zs", "600519", "招商银行", "CNY", "sh");
    insert_instrument_with_market(&conn, "inst-wk", "000002", "万科物业", "CNY", "sz");
    insert_instrument_with_market(&conn, "inst-abc", "ABCH", "ABC银行", "CNY", "sh");

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
    insert_instrument_with_market(&conn, "inst-held", "HELD", "持仓标的", "USD", "sh");
    // 已清仓：买入 10 股后全部卖出
    insert_instrument_with_market(&conn, "inst-closed", "CLOSED", "已清仓标的", "USD", "sz");
    // 未投资：从未交易
    insert_instrument_with_market(&conn, "inst-never", "NEVER", "未投资标的", "USD", "hk");

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
    insert_instrument_with_market(&conn, "inst-held", "HELD", "持仓标的", "USD", "sh");
    insert_instrument_with_market(&conn, "inst-closed", "CLOSED", "已清仓标的", "USD", "sz");
    insert_instrument_with_market(&conn, "inst-never", "NEVER", "未投资标的", "USD", "hk");

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
    insert_instrument_with_market(&conn, "inst-del", "DELHOLD", "删除账户持仓", "USD", "sh");

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
        only_invested: Some(true),
        page: None,
        page_size: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 0);
}

#[test]
fn buy_transaction_creates_lot() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-buy", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-test-nvda", "NVDA", "NVIDIA", "USD");

    let input = make_buy_input("acc-test-buy", "inst-test-nvda", 10.0, 10000, 500);
    let txn_id = create_transaction_internal(&conn, input).unwrap();

    let (kind, amount_cents, currency_code, amount_native, category_id, refund_of_id): (
        String,
        i64,
        String,
        i64,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT kind, amount_cents, currency_code, amount_native_cents, category_id, \
             refund_of_transaction_id FROM transactions WHERE id=?1",
            params![txn_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(kind, "buy");
    assert_eq!(amount_cents, 100500);
    assert_eq!(currency_code, "USD");
    assert_eq!(amount_native, amount_cents, "买入本位币与原始币种应 1:1");
    assert_eq!(category_id, None);
    assert_eq!(refund_of_id, None);

    let (action, quantity, price_cents, fee_cents): (String, f64, i64, i64) = conn
        .query_row(
            "SELECT action, quantity, price_cents, fee_cents FROM security_transactions WHERE transaction_id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(action, "buy");
    assert!((quantity - 10.0).abs() < 0.0001);
    assert_eq!(price_cents, 10000);
    assert_eq!(fee_cents, 500);

    let (remaining_quantity, cost_per_unit): (f64, i64) = conn
        .query_row(
            "SELECT remaining_quantity, cost_per_unit_cents FROM security_lots WHERE buy_transaction_id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!((remaining_quantity - 10.0).abs() < 0.0001);
    assert_eq!(cost_per_unit, 10050);

    let (holding_quantity, cost_basis): (f64, i64) = conn
        .query_row(
            "SELECT quantity, cost_basis_cents FROM v_holdings WHERE id=?1",
            params!["acc-test-buy-inst-test-nvda-USD"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!((holding_quantity - 10.0).abs() < 0.0001);
    assert_eq!(cost_basis, 100500);
}

/// buy 本位币金额经 Amount 接缝折算到全局默认币种（issue #70）：非 1:1 汇率下
/// 落库的 `amount_native_cents` 为折算值而非原始金额（旧行为硬编码 1:1）。
#[test]
fn buy_native_cents_converted_via_amount_seam() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-conv", "美股", "investment", "USD");
    insert_rate(&conn, "USD", "CNY", 7.2);
    insert_instrument(&conn, "inst-test-conv", "NVDA", "NVIDIA", "USD");

    let input = make_buy_input("acc-test-conv", "inst-test-conv", 10.0, 10000, 500);
    let txn_id = create_transaction_internal(&conn, input).unwrap();

    let (amount_cents, amount_native_cents): (i64, i64) = conn
        .query_row(
            "SELECT amount_cents, amount_native_cents FROM transactions WHERE id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(amount_cents, 100500, "原始币种金额 = 数量×单价+手续费");
    assert_eq!(
        amount_native_cents, 723600,
        "本位币金额应经 convert_to_native 折算（100500 × 7.2）"
    );
}

/// 修改 buy 交易（行为层 revert→plan→apply 的 UPDATE 侧）同样经折算：非 1:1 汇率下
/// `amount_native_cents` 保持折算值（INSERT/UPDATE 共用 prepare，防回归）。
#[test]
fn buy_update_native_cents_converted_via_amount_seam() {
    use crate::commands::transactions::update_transaction_internal;
    let conn = setup_db();
    insert_account(&conn, "acc-test-conv-upd", "美股", "investment", "USD");
    insert_rate(&conn, "USD", "CNY", 7.2);
    insert_instrument(&conn, "inst-test-conv-upd", "NVDA", "NVIDIA", "USD");

    let txn_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-conv-upd", "inst-test-conv-upd", 10.0, 10000, 500),
    )
    .unwrap();

    let mut edited = make_buy_input("acc-test-conv-upd", "inst-test-conv-upd", 5.0, 12000, 0);
    edited.date = "2026-02-01".into();
    update_transaction_internal(&conn, &txn_id, edited).unwrap();

    let (amount_cents, amount_native_cents): (i64, i64) = conn
        .query_row(
            "SELECT amount_cents, amount_native_cents FROM transactions WHERE id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(amount_cents, 60000, "修改后金额 = 5×12000");
    assert_eq!(
        amount_native_cents, 432000,
        "修改后本位币金额应经 convert_to_native 折算（60000 × 7.2）"
    );
}

#[test]
fn buy_transaction_requires_investment_account() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-cash", "现金", "cash", "CNY");
    insert_instrument(&conn, "inst-test-cny", "600519", "茅台", "CNY");

    let input = make_buy_input("acc-test-cash", "inst-test-cny", 1.0, 10000, 0);
    let err = create_transaction_internal(&conn, input).unwrap_err();
    assert!(
        err.to_string().contains("投资账户"),
        "非投资账户买入应报错，got: {err}"
    );
}

#[test]
fn sell_transaction_matches_multiple_lots_fifo() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-sell", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-test-sell", "TSLA", "Tesla", "USD");

    let lot1_txn = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-sell", "inst-test-sell", 10.0, 10000, 0),
    )
    .unwrap();
    let lot2_txn = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-sell", "inst-test-sell", 5.0, 12000, 0),
    )
    .unwrap();

    conn.execute(
        "UPDATE security_lots SET created_at='2026-01-10T00:00:00Z' WHERE buy_transaction_id=?1",
        params![lot1_txn],
    )
    .unwrap();
    conn.execute(
        "UPDATE security_lots SET created_at='2026-01-15T00:00:00Z' WHERE buy_transaction_id=?1",
        params![lot2_txn],
    )
    .unwrap();

    let sell_txn = create_transaction_internal(
        &conn,
        make_sell_input("acc-test-sell", "inst-test-sell", 12.0, 15000, 600),
    )
    .unwrap();

    let (kind, amount_cents): (TransactionKind, i64) = conn
        .query_row(
            "SELECT kind, amount_cents FROM transactions WHERE id=?1",
            params![sell_txn],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(kind, TransactionKind::Sell);
    assert_eq!(amount_cents, 179400);

    let rem1: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![lot1_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rem1 - 0.0).abs() < 0.0001);
    let rem2: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![lot2_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rem2 - 3.0).abs() < 0.0001);

    let rows: Vec<(f64, i64, i64, String)> = conn
        .prepare(
            "SELECT quantity, cost_per_unit_cents, realized_pnl_cents, currency_code \
             FROM security_lot_sales WHERE sell_transaction_id=?1 ORDER BY quantity DESC",
        )
        .unwrap()
        .query_map(params![sell_txn], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(rows.len(), 2);
    assert!((rows[0].0 - 10.0).abs() < 0.0001);
    assert_eq!(rows[0].1, 10000);
    assert_eq!(rows[0].2, 49500);
    assert_eq!(rows[0].3, "USD");
    assert!((rows[1].0 - 2.0).abs() < 0.0001);
    assert_eq!(rows[1].1, 12000);
    assert_eq!(rows[1].2, 5900);
    assert_eq!(rows[1].3, "USD");
}

#[test]
fn sell_transaction_rejects_oversell() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-oversell", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-test-oversell", "MSFT", "Microsoft", "USD");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-test-oversell", "inst-test-oversell", 5.0, 10000, 0),
    )
    .unwrap();

    let sell = make_sell_input("acc-test-oversell", "inst-test-oversell", 6.0, 12000, 0);
    assert!(create_transaction_internal(&conn, sell).is_err());
}

#[test]
fn sell_transaction_pnl_deducts_fee() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-pnl", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-test-pnl", "AAPL", "Apple", "USD");

    let buy_txn = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-pnl", "inst-test-pnl", 10.0, 10000, 0),
    )
    .unwrap();
    let sell_txn = create_transaction_internal(
        &conn,
        make_sell_input("acc-test-pnl", "inst-test-pnl", 5.0, 12000, 200),
    )
    .unwrap();

    let rem: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rem - 5.0).abs() < 0.0001);

    let (qty, cost, pnl, ccy): (f64, i64, i64, String) = conn
        .query_row(
            "SELECT quantity, cost_per_unit_cents, realized_pnl_cents, currency_code \
             FROM security_lot_sales WHERE sell_transaction_id=?1",
            params![sell_txn],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert!((qty - 5.0).abs() < 0.0001);
    assert_eq!(cost, 10000);
    assert_eq!(pnl, 9800);
    assert_eq!(ccy, "USD");

    let amount_cents: i64 = conn
        .query_row(
            "SELECT amount_cents FROM transactions WHERE id=?1",
            params![sell_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(amount_cents, 5 * 12000 - 200);
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

    let buy_input = make_buy_input("acc-hold", "inst-hold", 10.0, 15000, 1000);
    create_transaction_internal(&conn, buy_input).unwrap();

    let now = crate::db::now_iso();
    let price_id = crate::db::new_uuid();
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,16000,'USD',?3,NULL,?4,?5,?6,?7)",
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

fn empty_filter() -> PnlFilter {
    PnlFilter {
        account_id: None,
        instrument_id: None,
    }
}

#[test]
fn realized_pnl_summary_empty_when_no_sales() {
    let conn = setup_db();
    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.total_realized_pnl_cents, 0);
    assert!(result.by_year.is_empty());
    assert!(result.by_account.is_empty());
    assert!(result.by_instrument.is_empty());
    assert!(result.details.is_empty());
}

#[test]
fn realized_pnl_summary_aggregates_single_sale() {
    let conn = setup_db();
    insert_account(&conn, "acc-pnl", "美股账户", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-pnl", "AAPL", "Apple", "USD");

    let _buy =
        create_transaction_internal(&conn, make_buy_input("acc-pnl", "inst-pnl", 10.0, 10000, 0))
            .unwrap();
    let _sell = create_transaction_internal(
        &conn,
        make_sell_input("acc-pnl", "inst-pnl", 5.0, 12000, 200),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(result.total_realized_pnl_cents, 9800);
    assert_eq!(result.by_year.len(), 1);
    assert_eq!(result.by_year[0].realized_pnl_cents, 9800);
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].account_id, "acc-pnl");
    assert_eq!(result.by_account[0].realized_pnl_cents, 9800);
    assert_eq!(result.by_instrument.len(), 1);
    assert_eq!(result.by_instrument[0].instrument_id, "inst-pnl");
    assert_eq!(result.by_instrument[0].symbol, "AAPL");
    assert_eq!(result.by_instrument[0].realized_pnl_cents, 9800);
    assert_eq!(result.details.len(), 1);
    assert_eq!(result.details[0].instrument_symbol, "AAPL");
    assert_eq!(result.details[0].quantity, 5.0);
    assert_eq!(result.details[0].realized_pnl_cents, 9800);
}

#[test]
fn realized_pnl_summary_aggregates_multiple_accounts() {
    let conn = setup_db();
    insert_account(&conn, "acc-a", "账户A", "investment", "USD");
    insert_account(&conn, "acc-b", "账户B", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-xyz", "XYZ", "Test Corp", "USD");

    create_transaction_internal(&conn, make_buy_input("acc-a", "inst-xyz", 10.0, 1000, 0)).unwrap();
    create_transaction_internal(&conn, make_buy_input("acc-b", "inst-xyz", 5.0, 2000, 0)).unwrap();
    create_transaction_internal(&conn, make_sell_input("acc-a", "inst-xyz", 4.0, 1500, 0)).unwrap();
    create_transaction_internal(&conn, make_sell_input("acc-b", "inst-xyz", 2.0, 2500, 0)).unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(result.total_realized_pnl_cents, 3000);
    assert_eq!(result.by_account.len(), 2);
    assert_eq!(result.by_account[0].account_id, "acc-a");
    assert_eq!(result.by_account[0].realized_pnl_cents, 2000);
    assert_eq!(result.by_account[1].account_id, "acc-b");
    assert_eq!(result.by_account[1].realized_pnl_cents, 1000);
    assert_eq!(result.details.len(), 2);
}

#[test]
fn realized_pnl_summary_filter_by_account() {
    let conn = setup_db();
    insert_account(&conn, "acc-a", "账户A", "investment", "USD");
    insert_account(&conn, "acc-b", "账户B", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-xyz", "XYZ", "Test Corp", "USD");

    create_transaction_internal(&conn, make_buy_input("acc-a", "inst-xyz", 10.0, 1000, 0)).unwrap();
    create_transaction_internal(&conn, make_buy_input("acc-b", "inst-xyz", 5.0, 2000, 0)).unwrap();
    create_transaction_internal(&conn, make_sell_input("acc-a", "inst-xyz", 4.0, 1500, 0)).unwrap();
    create_transaction_internal(&conn, make_sell_input("acc-b", "inst-xyz", 2.0, 2500, 0)).unwrap();

    let filter = PnlFilter {
        account_id: Some("acc-a".into()),
        instrument_id: None,
    };
    let result = query_realized_pnl_summary(&conn, &filter).unwrap();

    assert_eq!(result.total_realized_pnl_cents, 2000);
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.details.len(), 1);
}

#[test]
fn realized_pnl_summary_filter_by_instrument() {
    let conn = setup_db();
    insert_account(&conn, "acc-pnl", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-a", "AAPL", "Apple", "USD");
    insert_instrument(&conn, "inst-b", "GOOGL", "Alphabet", "USD");

    create_transaction_internal(&conn, make_buy_input("acc-pnl", "inst-a", 10.0, 1000, 0)).unwrap();
    create_transaction_internal(&conn, make_buy_input("acc-pnl", "inst-b", 5.0, 2000, 0)).unwrap();
    create_transaction_internal(&conn, make_sell_input("acc-pnl", "inst-a", 4.0, 1500, 0)).unwrap();
    create_transaction_internal(&conn, make_sell_input("acc-pnl", "inst-b", 2.0, 2500, 0)).unwrap();

    let filter = PnlFilter {
        account_id: None,
        instrument_id: Some("inst-a".into()),
    };
    let result = query_realized_pnl_summary(&conn, &filter).unwrap();

    assert_eq!(result.total_realized_pnl_cents, 2000);
    assert_eq!(result.by_instrument.len(), 1);
    assert_eq!(result.by_instrument[0].instrument_id, "inst-a");
    assert_eq!(result.details.len(), 1);
}

// ---------------------------------------------------------------------------
// 走势查询（issue #138 / spec #135 / ADR-0019）
// ---------------------------------------------------------------------------

/// 直插一条价格历史周点行（走势查询为只读命令，绕过采集通道直接铺样例数据）。
fn insert_price_history(
    conn: &Connection,
    id: &str,
    instrument_id: &str,
    trade_date: &str,
    price_cents: i64,
    currency: &str,
) {
    conn.execute(
        "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, instrument_id, trade_date, price_cents, currency],
    )
    .unwrap();
}

/// 直插一条汇率历史周点行（1 base = rate quote）。
fn insert_fx_rate_history(
    conn: &Connection,
    id: &str,
    base: &str,
    quote: &str,
    trade_date: &str,
    rate: f64,
) {
    conn.execute(
        "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, base, quote, trade_date, rate],
    )
    .unwrap();
}

/// 日期可指定的 buy/sell 输入（数量推算测试需要错开周采样日）。
fn make_trade_input(
    kind: TransactionKind,
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        kind,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: date.into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(0),
        idempotency_key: None,
    }
}

#[test]
fn instrument_price_trend_clips_range_and_starts_at_first_point() {
    let conn = setup_db();
    insert_instrument(&conn, "inst-t1", "600519", "贵州茅台", "CNY");
    insert_price_history(&conn, "ph-1", "inst-t1", "2026-01-05", 10000, "CNY");
    insert_price_history(&conn, "ph-2", "inst-t1", "2026-01-12", 11000, "CNY");
    insert_price_history(&conn, "ph-3", "inst-t1", "2026-01-19", 12000, "CNY");
    insert_price_history(&conn, "ph-4", "inst-t1", "2026-02-02", 13000, "CNY");

    // 区间裁剪：只返回区间内（含端点）的周点。
    let trend = trend::query_instrument_price_trend(
        &conn,
        "inst-t1",
        &TrendRange {
            start_date: Some("2026-01-10".into()),
            end_date: Some("2026-01-31".into()),
        },
    )
    .unwrap();
    let dates: Vec<&str> = trend.points.iter().map(|p| p.date.as_str()).collect();
    assert_eq!(dates, ["2026-01-12", "2026-01-19"]);
    assert_eq!(trend.points[0].price_cents, 11000);
    assert_eq!(trend.points[0].currency_code, "CNY");
    assert_eq!(trend.instrument_id, "inst-t1");

    // 不设界（"全部"区间）：从首个有效采样点开始，升序完整返回。
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-t1", &TrendRange::default()).unwrap();
    assert_eq!(trend.points.len(), 4);
    assert_eq!(trend.points[0].date, "2026-01-05");

    // 区间参数非法时报错，不静默返回曲线。
    let err = trend::query_instrument_price_trend(
        &conn,
        "inst-t1",
        &TrendRange {
            start_date: Some("2026-13-01".into()),
            end_date: None,
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Invalid(_)));
    let err = trend::query_instrument_price_trend(
        &conn,
        "inst-t1",
        &TrendRange {
            start_date: Some("2026-02-01".into()),
            end_date: Some("2026-01-01".into()),
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Invalid(_)));
}

#[test]
fn portfolio_trend_derives_quantity_from_buy_sell_flow() {
    let conn = setup_db();
    insert_account(&conn, "acc-trd", "证券户", "investment", "CNY");
    insert_instrument(&conn, "inst-t2", "000001", "平安银行", "CNY");
    // 周价格点：w1=1000、w2=2000、w3=3000、w4=4000（CNY，无需折算）。
    insert_price_history(&conn, "ph-w1", "inst-t2", "2026-02-02", 1000, "CNY");
    insert_price_history(&conn, "ph-w2", "inst-t2", "2026-02-09", 2000, "CNY");
    insert_price_history(&conn, "ph-w3", "inst-t2", "2026-02-16", 3000, "CNY");
    insert_price_history(&conn, "ph-w4", "inst-t2", "2026-02-23", 4000, "CNY");
    // 时序：w1 未买入（数量 0）→ w2 周内（02-06）买入 10 股 → w3 持有 10 股 → 2026-02-20（w3 内）清仓 → w4 归零。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-trd",
            "inst-t2",
            10.0,
            1500,
            "2026-02-06",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-trd",
            "inst-t2",
            10.0,
            3500,
            "2026-02-20",
        ),
    )
    .unwrap();

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.currency_code, "CNY");
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    assert_eq!(
        values,
        [
            ("2026-02-02".to_string(), 0),     // 买入前：价格有效但持有为零
            ("2026-02-09".to_string(), 20000), // 10 × 2000
            ("2026-02-16".to_string(), 30000), // 10 × 3000（卖出在 02-20，尚未生效）
            ("2026-02-23".to_string(), 0),     // 清仓后归零
        ]
    );
}

#[test]
fn portfolio_trend_with_date_range_clips_weeks_and_does_not_lose_pre_start_flow() {
    let conn = setup_db();
    insert_account(&conn, "acc-rng", "区间户", "investment", "CNY");
    insert_instrument(&conn, "inst-rng", "600036", "招商银行", "CNY");
    insert_price_history(&conn, "ph-r1", "inst-rng", "2026-04-06", 1000, "CNY");
    insert_price_history(&conn, "ph-r2", "inst-rng", "2026-04-13", 2000, "CNY");
    insert_price_history(&conn, "ph-r3", "inst-rng", "2026-04-20", 4000, "CNY");
    // 买入在区间起点之前：起点前的流水必须累积带入，起点后各周数量才非零。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-rng",
            "inst-rng",
            3.0,
            500,
            "2026-04-08",
        ),
    )
    .unwrap();

    // 回归（#138 评审）：带 start_date 的组合走势查询曾因流水查询占位符
    // 与参数个数不匹配而运行时报错；此处同时锁定区间裁剪与起点前持仓带入。
    let trend = trend::query_portfolio_value_trend(
        &conn,
        &TrendRange {
            start_date: Some("2026-04-10".into()),
            end_date: Some("2026-04-21".into()),
        },
    )
    .unwrap();
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    assert_eq!(
        values,
        [
            ("2026-04-13".to_string(), 6000),  // 3 × 2000（起点前买入已带入）
            ("2026-04-20".to_string(), 12000), // 3 × 4000
        ]
    );
}

#[test]
fn portfolio_trend_converts_hkd_via_same_week_fx_with_reverse_fallback() {
    let conn = setup_db();
    insert_account(&conn, "acc-hkd", "港美股户", "investment", "CNY");
    insert_instrument(&conn, "inst-hkd", "00700", "腾讯控股", "HKD");
    // 港股以 HKD 计价：w1=100 HKD（10000 分）、w2=200 HKD、w3=300 HKD。
    insert_price_history(&conn, "ph-h1", "inst-hkd", "2026-03-02", 10000, "HKD");
    insert_price_history(&conn, "ph-h2", "inst-hkd", "2026-03-09", 20000, "HKD");
    insert_price_history(&conn, "ph-h3", "inst-hkd", "2026-03-16", 30000, "HKD");
    // w1 有正向汇率 HKD->CNY=0.8；w2 只有反向 CNY->HKD=5.0（兜底取倒数 0.2）；w3 无任何历史汇率。
    insert_fx_rate_history(&conn, "fx-h1", "HKD", "CNY", "2026-03-03", 0.8);
    insert_fx_rate_history(&conn, "fx-h2", "CNY", "HKD", "2026-03-10", 5.0);
    // 2 股，全程持有（买入早于首条价格点）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-hkd",
            "inst-hkd",
            2.0,
            10000,
            "2026-02-20",
        ),
    )
    .unwrap();

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.currency_code, "CNY");
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    // w1: 2×10000×0.8=16000；w2: 2×20000×(1/5.0)=8000；w3 缺同期汇率 → 该周被跳过（不伪造数据）。
    assert_eq!(
        values,
        [
            ("2026-03-02".to_string(), 16000),
            ("2026-03-09".to_string(), 8000),
        ]
    );
}

#[test]
fn portfolio_trend_skips_weeks_missing_price_or_fx_but_keeps_other_contributors() {
    let conn = setup_db();
    insert_account(&conn, "acc-mix", "混合户", "investment", "CNY");
    insert_instrument(&conn, "inst-a", "600000", "浦发银行", "CNY");
    insert_instrument(&conn, "inst-b", "09988", "阿里巴巴", "HKD");
    // 各买 1 股，早于首条价格点。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-mix",
            "inst-a",
            1.0,
            1000,
            "2026-02-20",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-mix",
            "inst-b",
            1.0,
            10000,
            "2026-02-20",
        ),
    )
    .unwrap();
    // inst-a（CNY）三周全有价：1000 分/周。
    insert_price_history(&conn, "ph-a1", "inst-a", "2026-03-02", 1000, "CNY");
    insert_price_history(&conn, "ph-a2", "inst-a", "2026-03-09", 1000, "CNY");
    insert_price_history(&conn, "ph-a3", "inst-a", "2026-03-16", 1000, "CNY");
    // inst-b（HKD）w2 整周无价（停牌语义）；w3 有价但缺同期汇率。
    insert_price_history(&conn, "ph-b1", "inst-b", "2026-03-02", 10000, "HKD");
    insert_price_history(&conn, "ph-b3", "inst-b", "2026-03-16", 10000, "HKD");
    // 仅 w1 有 HKD->CNY=0.9。
    insert_fx_rate_history(&conn, "fx-m1", "HKD", "CNY", "2026-03-03", 0.9);

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    // w1: 1000 + 10000×0.9=10000；w2: inst-b 缺价被跳过，仅 inst-a 1000；w3: inst-b 缺汇率被跳过，仅 inst-a 1000。
    assert_eq!(
        values,
        [
            ("2026-03-02".to_string(), 10000),
            ("2026-03-09".to_string(), 1000),
            ("2026-03-16".to_string(), 1000),
        ]
    );
}

#[test]
fn trend_commands_return_empty_state_without_history() {
    let conn = setup_db();
    insert_instrument(&conn, "inst-empty", "000002", "万科A", "CNY");

    // 无任何价格历史：单标的与组合走势都返回空态结构（points 为空）。
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-empty", &TrendRange::default()).unwrap();
    assert_eq!(trend.instrument_id, "inst-empty");
    assert!(trend.points.is_empty());

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.currency_code, "CNY");
    assert!(trend.points.is_empty());
}

#[test]
fn get_transaction_trade_returns_buy_detail_with_instrument_display() {
    let conn = setup_db();
    insert_account(&conn, "acc-inv", "证券户", "investment", "USD");
    insert_instrument(&conn, "inst-t", "600519", "贵州茅台", "USD");
    insert_rate_1_1(&conn, "USD");
    let id =
        create_transaction_internal(&conn, make_buy_input("acc-inv", "inst-t", 100.0, 1500, 500))
            .unwrap();

    let trade = trade::get_transaction_trade(&conn, &id).unwrap();
    assert_eq!(trade.instrument_id, "inst-t");
    assert_eq!(trade.symbol, "600519");
    assert_eq!(trade.instrument_name.as_deref(), Some("贵州茅台"));
    assert!((trade.quantity - 100.0).abs() < 1e-9);
    assert_eq!(trade.price_cents, 1500);
    assert_eq!(trade.fee_cents, Some(500));
}

#[test]
fn get_transaction_trade_rejects_missing_or_non_trade_transaction() {
    let conn = setup_db();
    insert_account(&conn, "acc-cash", "现金", "cash", "CNY");
    // 非买卖交易（expense）无买卖明细
    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: None,
            kind: TransactionKind::Expense,
            amount_cents: 1000,
            currency_code: "CNY".into(),
            account_id: "acc-cash".into(),
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-10".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        },
    )
    .unwrap();
    let err = trade::get_transaction_trade(&conn, &expense_id).unwrap_err();
    assert!(err.to_string().contains("无买卖明细"), "实际: {err}");
    // 不存在的 id 同样 NotFound
    let err = trade::get_transaction_trade(&conn, "no-such-txn").unwrap_err();
    assert!(err.to_string().contains("无买卖明细"), "实际: {err}");
}
