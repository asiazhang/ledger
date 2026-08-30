//! 持仓价格增量同步（HoldingPriceSync，issue #103 / #137 / ADR-0019）：secid 构造、
//! ulist / 日 K / 汇率 K 报文解析、现价 upsert、K 线周采样回填与幂等语义。
//! 编排经注入 mock 查询 / kline / fx 闭包驱动，不依赖真实网络。

use std::cell::RefCell;

use rusqlite::{Connection, params};

use crate::commands::sync::fund_nav::{LsjzPage, NavPoint, NavQuery};
use crate::commands::sync::http::{
    KlineBar, KlineResponse, StockItem, ULIST_BATCH_SIZE, UlistResponse, f2_to_price,
    fx_secid_candidates, parse_klines, secid_prefix,
};
use crate::commands::sync::incremental::{beijing_today, do_incremental_sync_with};
use crate::commands::sync::persist::{
    EASTMONEY_PRICE_SOURCE, upsert_market_price, upsert_price_history,
};
use crate::error::{AppError, Result};

use super::common::setup_db;

// ---------------------------------------------------------------------------
// 持仓价格增量同步（issue #103）：secid 构造、ulist 响应解析、编排、跳过规则、
// 结果统计与幂等。编排经注入 mock 查询函数驱动，不依赖真实网络。
// ---------------------------------------------------------------------------

/// 直插一条持仓（账户 + 标的 + 交易 + 批次），绕过交易行为层以聚焦增量同步自身逻辑。
fn insert_account(conn: &Connection, id: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'investment',?3,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![id, format!("账户-{id}"), currency],
    )
    .unwrap();
}

fn insert_instrument(
    conn: &Connection,
    id: &str,
    symbol: &str,
    kind: &str,
    currency: &str,
    market: &str,
) {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, symbol, kind, format!("名称-{symbol}"), currency, market],
    )
    .unwrap();
}

fn insert_lot(conn: &Connection, account_id: &str, instrument_id: &str, currency: &str) {
    let txn_id = format!("txn-{account_id}-{instrument_id}");
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id) \
         VALUES (?1,'buy',1000,?2,1000,?3,'2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params![txn_id, currency, account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',10,100,0)",
        params![txn_id, instrument_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,10,10,100,?5,'2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params![
            format!("lot-{account_id}-{instrument_id}"),
            account_id,
            instrument_id,
            txn_id,
            currency
        ],
    )
    .unwrap();
}

/// 组合帮手：账户 + 标的 + 持仓批次一步建好。
fn insert_holding(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
    symbol: &str,
    kind: &str,
    currency: &str,
    market: &str,
) {
    insert_account(conn, account_id, currency);
    insert_instrument(conn, instrument_id, symbol, kind, currency, market);
    insert_lot(conn, account_id, instrument_id, currency);
}

fn market_price_of(conn: &Connection, instrument_id: &str) -> Option<i64> {
    conn.query_row(
        "SELECT price_cents FROM market_prices WHERE instrument_id=?1",
        params![instrument_id],
        |r| r.get(0),
    )
    .ok()
}

/// 模拟批量报价：对每个查询的 secid 生成条目。`prices` 为 code → 原始 f2
/// （None 表示停牌/无效价），不在映射中的代码不返回（模拟查询无果）。
fn mock_fetch<'a>(
    prices: &'a [(&'a str, Option<f64>)],
) -> impl FnMut(&str) -> Result<Vec<StockItem>> + 'a {
    move |secids: &str| {
        let mut items = Vec::new();
        for secid in secids.split(',') {
            let code = secid.split('.').nth(1).unwrap_or(secid).to_string();
            if let Some((_, price)) = prices.iter().find(|(c, _)| *c == code) {
                items.push(StockItem {
                    name: format!("名称-{code}"),
                    code,
                    price: *price,
                });
            }
        }
        Ok(items)
    }
}

#[test]
fn secid_prefix_maps_known_markets() {
    assert_eq!(secid_prefix("sh"), Some("1"));
    assert_eq!(secid_prefix("sz"), Some("0"));
    assert_eq!(secid_prefix("hk"), Some("116"));
    assert_eq!(secid_prefix("unknown"), None);
}

#[test]
fn ulist_response_deserializes_cross_market_codes() {
    // 真实 ulist.np/get 响应样本（一次携带跨市场：沪 1.600519 / 深 0.000001 / 港 116.00700）
    let json = r#"{"rc":0,"rt":11,"svr":177542529,"lt":1,"full":1,"dlmkts":"8,10,128","dsc":"0","data":{"total":3,"diff":[{"f2":130280,"f12":"600519","f14":"贵州茅台"},{"f2":1173,"f12":"000001","f14":"平安银行"},{"f2":445400,"f12":"00700","f14":"腾讯控股"}]}}"#;
    let resp: UlistResponse = serde_json::from_str(json).unwrap();
    let items = resp.data.unwrap().diff.unwrap().into_items();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].code, "600519");
    // 价格换算（万分之一元，ADR-0038）：A 股 f2 × 100、港股 × 10（与全量同步一致）
    assert_eq!(f2_to_price(items[0].price.unwrap(), "sh"), 13028000);
    assert_eq!(f2_to_price(items[1].price.unwrap(), "sz"), 117300);
    assert_eq!(f2_to_price(items[2].price.unwrap(), "hk"), 4454000);
}

#[test]
fn ulist_response_null_data_yields_no_items() {
    // 全部代码无效时东财返回 rc=102 且 data:null：应解析为空而非报错（不中断同步）。
    let json = r#"{"rc":102,"rt":1,"svr":177622402,"lt":1,"full":1,"dlmkts":"8,10,128","dsc":"0","data":null}"#;
    let resp: UlistResponse = serde_json::from_str(json).unwrap();
    assert!(resp.data.is_none());
}

#[test]
fn incremental_sync_normalizes_symbol_suffix() {
    let conn = setup_db();
    // schema 注释示例格式：symbol 带市场后缀（"600519.SH"），secid 应取裸代码 "1.600519"。
    insert_holding(&conn, "acc-1", "inst-sh", "600519.SH", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-hk", "00700.HK", "stock", "HKD", "hk");

    // mock 按响应侧裸代码（f12）返回：归一化后应能匹配并写入价格。
    let prices = [("600519", Some(130280.0)), ("00700", Some(445400.0))];
    let mut fetch = mock_fetch(&prices);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut no_nav)
            .unwrap();

    assert_eq!(result.synced, 2);
    assert_eq!(result.skipped, 0);
    assert_eq!(
        market_price_of(&conn, "inst-sh"),
        Some(13028000),
        "A 股 f2 × 100 得万分之一元"
    );
    assert_eq!(
        market_price_of(&conn, "inst-hk"),
        Some(4454000),
        "港股 f2 × 10 得万分之一元"
    );
}

#[test]
fn incremental_sync_all_missing_response_counts_all_skipped() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-a", "600001", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-b", "600002", "stock", "CNY", "sh");

    // 查询全部无果（如整批代码无效、响应 data:null）：不报错、全部计入跳过。
    let prices: [(&str, Option<f64>); 0] = [];
    let mut fetch = mock_fetch(&prices);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut no_nav)
            .unwrap();

    assert_eq!(result.synced, 0);
    assert_eq!(result.skipped, 2);
    assert_eq!(market_price_of(&conn, "inst-a"), None);
    assert_eq!(market_price_of(&conn, "inst-b"), None);
}

#[test]
fn incremental_sync_no_holdings_returns_message() {
    let conn = setup_db();
    let mut fetch = mock_fetch(&[]);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut no_nav)
            .unwrap();
    assert_eq!(result.synced, 0);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.message, "无持仓标的可同步");
}

#[test]
fn incremental_sync_updates_holding_prices_only() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-sz", "000001", "stock", "CNY", "sz");
    insert_holding(&conn, "acc-3", "inst-hk", "00700", "stock", "HKD", "hk");
    // 预先存在的旧价（应被覆盖更新，不产生新行）
    upsert_market_price(
        &conn,
        "inst-sh",
        999,
        "CNY",
        "2026-01-01T00:00:00Z",
        None,
        Some("eastmoney"),
    )
    .unwrap();

    let prices = [
        ("600519", Some(130280.0)),
        ("000001", Some(1173.0)),
        ("00700", Some(445400.0)),
    ];
    let mut fetch = mock_fetch(&prices);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut no_nav)
            .unwrap();

    assert_eq!(result.synced, 3);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.message, "已同步 3 只，跳过 0 只");

    // 价格覆盖更新：A 股直接得分、港股 ÷10
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(13028000));
    assert_eq!(market_price_of(&conn, "inst-sz"), Some(117300));
    assert_eq!(market_price_of(&conn, "inst-hk"), Some(4454000));
    // 每标的一条价格（无重复）
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);

    // 标的字典（名称/市场）不变
    let (name, market): (Option<String>, String) = conn
        .query_row(
            "SELECT name, market FROM instruments WHERE id='inst-sh'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(name.as_deref(), Some("名称-600519"));
    assert_eq!(market, "sh");
    // 未新增标的
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 3);
}

#[test]
fn incremental_sync_skips_holdings_without_quote_source() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(
        &conn,
        "acc-2",
        "inst-bond",
        "019547",
        "bond",
        "CNY",
        "unknown",
    );
    insert_holding(
        &conn,
        "acc-3",
        "inst-other",
        "稳稳地幸福",
        "other",
        "CNY",
        "unknown",
    );

    let prices = [("600519", Some(130280.0))];
    let mut fetch = mock_fetch(&prices);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut no_nav)
            .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(
        result.skipped, 2,
        "无行情来源类型（债券/其他）计入跳过统计（基金走净值通道另测）"
    );
    assert_eq!(result.message, "已同步 1 只，跳过 2 只");
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(13028000));
    assert_eq!(
        market_price_of(&conn, "inst-bond"),
        None,
        "无行情来源持仓不写价格"
    );
    assert_eq!(market_price_of(&conn, "inst-other"), None);
}

#[test]
fn incremental_sync_keeps_old_price_when_suspended() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-sz", "000001", "stock", "CNY", "sz");
    // 停牌股已有旧价
    upsert_market_price(
        &conn,
        "inst-sz",
        888,
        "CNY",
        "2026-01-01T00:00:00Z",
        None,
        Some("eastmoney"),
    )
    .unwrap();

    // 600519 正常价；000001 停牌（f2 无效 → None）
    let prices = [("600519", Some(130280.0)), ("000001", None)];
    let mut fetch = mock_fetch(&prices);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut no_nav)
            .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 1, "停牌/无效价应计入跳过且不中断同步");
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(13028000));
    assert_eq!(
        market_price_of(&conn, "inst-sz"),
        Some(888),
        "停牌应保留旧价"
    );
}

#[test]
fn incremental_sync_counts_missing_response_as_skipped() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-a", "600001", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-b", "600002", "stock", "CNY", "sh");

    // mock 只返回 600001：600002 查询无果（响应缺失）→ 计入跳过
    let prices = [("600001", Some(1000.0))];
    let mut fetch = mock_fetch(&prices);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut no_nav)
            .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 1);
    assert_eq!(market_price_of(&conn, "inst-a"), Some(100000));
    assert_eq!(market_price_of(&conn, "inst-b"), None);
}

#[test]
fn incremental_sync_skips_unknown_market() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-ok", "600519", "stock", "CNY", "sh");
    // 市场未知的持仓股票（如手动创建未设市场）：无法构造 secid，计入跳过
    insert_holding(
        &conn, "acc-2", "inst-unk", "NVDA", "stock", "USD", "unknown",
    );

    let prices = [("600519", Some(130280.0))];
    let mut fetch = mock_fetch(&prices);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut no_nav)
            .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 1, "市场未知应计入跳过");
    assert_eq!(market_price_of(&conn, "inst-unk"), None);
}

#[test]
fn incremental_sync_is_idempotent() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");

    let prices = [("600519", Some(130280.0))];
    let mut fetch = mock_fetch(&prices);
    let first = do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut no_nav)
        .unwrap();
    assert_eq!(first.synced, 1);

    let mut fetch = mock_fetch(&prices);
    let second =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut no_nav)
            .unwrap();
    assert_eq!(second.synced, 1);

    // 重复调用不产生重复价格行（每标的一条，覆盖更新）
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(13028000));
}

#[test]
fn incremental_sync_dedupes_same_instrument_across_accounts() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    // 同一标的在另一账户也有持仓：应去重为一只、只查一次
    insert_account(&conn, "acc-2", "CNY");
    insert_lot(&conn, "acc-2", "inst-sh", "CNY");

    let prices = [("600519", Some(1000.0))];
    let mut fetch = mock_fetch(&prices);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut no_nav)
            .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 0);
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(100000));
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn incremental_sync_batches_by_fifty() {
    let conn = setup_db();
    // 55 只股票：应拆为 2 批（50 + 5），每批 secid 数不超 ULIST_BATCH_SIZE
    for i in 0..55 {
        let symbol = format!("{:06}", 600000 + i);
        insert_holding(
            &conn,
            &format!("acc-{i}"),
            &format!("inst-{i}"),
            &symbol,
            "stock",
            "CNY",
            "sh",
        );
    }

    let mut batch_sizes: Vec<usize> = Vec::new();
    let mut fetch = |secids: &str| {
        let codes: Vec<&str> = secids.split(',').collect();
        assert!(codes.len() <= ULIST_BATCH_SIZE);
        batch_sizes.push(codes.len());
        Ok(codes
            .iter()
            .map(|secid| {
                let code = secid.split('.').nth(1).unwrap().to_string();
                StockItem {
                    code,
                    name: "名称".into(),
                    price: Some(1000.0),
                }
            })
            .collect())
    };
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut no_nav)
            .unwrap();

    assert_eq!(result.synced, 55);
    assert_eq!(batch_sizes, vec![50, 5]);
}

#[test]
fn incremental_sync_propagates_fetch_error() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");

    let mut fetch = |_: &str| Err(AppError::Io("模拟网络失败".into()));
    let err = do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut no_nav)
        .unwrap_err();
    assert!(err.to_string().contains("模拟网络失败"));
}

// ---------------------------------------------------------------------------
// K 线回填（issue #137 / ADR-0019）：现价 upsert 之外，近两年日 K 回填降采样落
// PriceHistory / FxRateHistory。编排经注入 mock kline / mock fx 闭包驱动
// （与批量报价同一接缝），不依赖真实网络。
// ---------------------------------------------------------------------------

/// 构造一根日 K 样本（日期为 ISO 日期，收盘为真实价格值，如 10.40 元）。
fn bar(date: &str, close: f64) -> KlineBar {
    KlineBar {
        date: date.to_string(),
        close,
    }
}

/// 模拟日 K 抓取：按完整 secid（如 "1.600519"）返回日线样本；未命中返回空
/// （模拟全段停牌 / 无效代码）。
fn mock_kline<'a>(
    by_secid: &'a [(&'a str, Vec<KlineBar>)],
) -> impl FnMut(&str) -> Result<Vec<KlineBar>> + 'a {
    move |secid: &str| {
        Ok(by_secid
            .iter()
            .find(|(s, _)| *s == secid)
            .map(|(_, bars)| bars.clone())
            .unwrap_or_default())
    }
}

/// 空实现：既有用例只关心现价行为时注入（历史回填接缝的最小桩）。
fn no_kline(_: &str) -> Result<Vec<KlineBar>> {
    Ok(vec![])
}

/// 空实现：同 [`no_kline`]，用于汇率回填。
fn no_fx(_: &str) -> Result<Vec<KlineBar>> {
    Ok(vec![])
}

/// 空实现：既有用例只关心股票/汇率行为时注入（净值通道最小桩，首刷查无净值
/// 形态——基金计入跳过）。
fn no_nav(_: &NavQuery) -> Result<LsjzPage> {
    Ok(LsjzPage {
        points: vec![],
        total: 0,
    })
}

/// 模拟汇率 K 线抓取：按 base+quote 直连串（如 "HKDCNY"）返回汇率日线样本，
/// 并记录被请求的币种对（断言只对非本位币发起抓取）。
fn mock_fx<'a>(
    by_pair: &'a [(&'a str, Vec<KlineBar>)],
    requested: &'a RefCell<Vec<String>>,
) -> impl FnMut(&str) -> Result<Vec<KlineBar>> + 'a {
    move |pair: &str| {
        requested.borrow_mut().push(pair.to_string());
        Ok(by_pair
            .iter()
            .find(|(p, _)| *p == pair)
            .map(|(_, bars)| bars.clone())
            .unwrap_or_default())
    }
}

/// 查询某标的的周采样价格历史（trade_date, price_cents, currency_code），按日期升序。
fn price_history_rows(conn: &Connection, instrument_id: &str) -> Vec<(String, i64, String)> {
    let mut stmt = conn
        .prepare(
            "SELECT trade_date, price_cents, currency_code FROM price_history \
             WHERE instrument_id=?1 ORDER BY trade_date",
        )
        .unwrap();
    let rows = stmt
        .query_map(params![instrument_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .unwrap();
    rows.map(|r| r.unwrap()).collect()
}

/// 查询币种对的周采样汇率历史（trade_date, rate），按日期升序。
fn fx_rows(conn: &Connection, base: &str, quote: &str) -> Vec<(String, f64)> {
    let mut stmt = conn
        .prepare(
            "SELECT trade_date, rate FROM fx_rate_history \
             WHERE base_code=?1 AND quote_code=?2 ORDER BY trade_date",
        )
        .unwrap();
    let rows = stmt
        .query_map(params![base, quote], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })
        .unwrap();
    rows.map(|r| r.unwrap()).collect()
}

#[test]
fn kline_backfill_downsamples_daily_to_weekly() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");

    // 一段跨年日线：2025-12-29 ~ 2026-01-04 属同一 ISO 周（跨年边界），
    // 元旦假期整周缺价跳点；其余周取最后一个有报价交易日。
    let bars = vec![
        bar("2025-12-24", 10.00), // 该周（12-22 起）最后一交易日
        bar("2025-12-29", 10.05), // 跨年周首个交易日
        bar("2025-12-31", 10.10), // 跨年周最后一交易日（1/1-1/4 假期）
        // 2026-01-05 ~ 01-09 整周节假日无报价：该周无点
        bar("2026-01-12", 10.30),
        bar("2026-01-13", 10.40), // 该周最后交易日
    ];
    let klines = [("1.600519", bars)];
    let prices = [("600519", Some(1040.0))];
    let fx_log = RefCell::new(Vec::new());
    let mut fetch = mock_fetch(&prices);
    let mut kline = mock_kline(&klines);
    let mut fx = mock_fx(&[], &fx_log);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut kline, &mut fx, &mut no_nav).unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(
        price_history_rows(&conn, "inst-sh"),
        vec![
            ("2025-12-24".into(), 100000, "CNY".into()),
            ("2025-12-31".into(), 101000, "CNY".into()),
            ("2026-01-13".into(), 104000, "CNY".into()),
        ],
        "每周取最后一个有报价交易日的收盘价；整周缺价该周无点"
    );
}

#[test]
fn kline_backfill_full_week_overwrite_is_idempotent() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    let prices = [("600519", Some(920.0))];
    let fx_log = RefCell::new(Vec::new());

    // 第一轮回填：该周最后交易日为周五 01-09。
    let first = [(
        "1.600519",
        vec![bar("2026-01-05", 9.00), bar("2026-01-09", 9.50)],
    )];
    // 第二轮回填：周五修正为缺价，最后交易日变为周四 01-08。
    let second = [(
        "1.600519",
        vec![bar("2026-01-05", 9.00), bar("2026-01-08", 9.20)],
    )];

    let mut fetch = mock_fetch(&prices);
    let mut kline = mock_kline(&first);
    let mut fx = mock_fx(&[], &fx_log);
    do_incremental_sync_with(&conn, &mut fetch, &mut kline, &mut fx, &mut no_nav).unwrap();

    let mut fetch = mock_fetch(&prices);
    let mut kline = mock_kline(&second);
    let mut fx = mock_fx(&[], &fx_log);
    do_incremental_sync_with(&conn, &mut fetch, &mut kline, &mut fx, &mut no_nav).unwrap();

    assert_eq!(
        price_history_rows(&conn, "inst-sh"),
        vec![("2026-01-08".into(), 92000, "CNY".into())],
        "同周重复回填整周覆盖：采样日与价格取最新一次抓取"
    );
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "重复回填零重复行");
}

#[test]
fn kline_backfill_keeps_history_after_position_cleared() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-sz", "000001", "stock", "CNY", "sz");

    let bars = [
        (
            "1.600519",
            vec![bar("2026-01-05", 10.00), bar("2026-01-06", 10.10)],
        ),
        (
            "0.000001",
            vec![bar("2026-01-05", 11.00), bar("2026-01-06", 11.20)],
        ),
    ];
    let prices = [("600519", Some(1010.0)), ("000001", Some(1120.0))];
    let fx_log = RefCell::new(Vec::new());

    let mut fetch = mock_fetch(&prices);
    let mut kline = mock_kline(&bars);
    let mut fx = mock_fx(&[], &fx_log);
    do_incremental_sync_with(&conn, &mut fetch, &mut kline, &mut fx, &mut no_nav).unwrap();
    assert_eq!(price_history_rows(&conn, "inst-sh").len(), 1);
    assert_eq!(price_history_rows(&conn, "inst-sz").len(), 1);

    // 清仓 inst-sh：删除持仓批次后不再参与采集。
    conn.execute(
        "DELETE FROM security_lots WHERE instrument_id='inst-sh'",
        [],
    )
    .unwrap();

    let mut fetch = mock_fetch(&prices);
    let mut kline = mock_kline(&bars);
    let mut fx = mock_fx(&[], &fx_log);
    do_incremental_sync_with(&conn, &mut fetch, &mut kline, &mut fx, &mut no_nav).unwrap();

    assert_eq!(
        price_history_rows(&conn, "inst-sh").len(),
        1,
        "清仓后历史保留不删"
    );
    assert_eq!(price_history_rows(&conn, "inst-sz").len(), 1);
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 2);
}

#[test]
fn kline_backfill_writes_fx_rate_history_alongside() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-hk", "00700", "stock", "HKD", "hk");

    let klines = [
        (
            "1.600519",
            vec![bar("2026-01-05", 10.00), bar("2026-01-13", 10.40)],
        ),
        (
            "116.00700",
            vec![bar("2026-01-05", 475.00), bar("2026-01-13", 480.00)],
        ),
    ];
    let prices = [("600519", Some(1040.0)), ("00700", Some(480000.0))];
    let fx_bars = [(
        "HKDCNY",
        vec![bar("2026-01-05", 0.91), bar("2026-01-13", 0.92)],
    )];
    let fx_log = RefCell::new(Vec::new());
    let mut fetch = mock_fetch(&prices);
    let mut kline = mock_kline(&klines);
    let mut fx = mock_fx(&fx_bars, &fx_log);
    do_incremental_sync_with(&conn, &mut fetch, &mut kline, &mut fx, &mut no_nav).unwrap();

    // 汇率与价格同期段（同周规则）落 FxRateHistory：base=HKD、quote=本位币 CNY。
    assert_eq!(
        fx_rows(&conn, "HKD", "CNY"),
        vec![("2026-01-05".into(), 0.91), ("2026-01-13".into(), 0.92)],
    );
    // 价格历史同期落库（收盘价 ×10000 得万分之一元：475.00 → 4750000，ADR-0038 刻度）。
    assert_eq!(
        price_history_rows(&conn, "inst-hk"),
        vec![
            ("2026-01-05".into(), 4750000, "HKD".into()),
            ("2026-01-13".into(), 4800000, "HKD".into()),
        ],
    );
    // 仅非本位币币种对触发汇率抓取（CNY 股票不查汇率）。
    assert_eq!(fx_log.borrow().as_slice(), ["HKDCNY"]);
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM fx_rate_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2, "每币种对每周至多一条");
}

#[test]
fn kline_backfill_empty_history_keeps_quote_only() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    let prices = [("600519", Some(1000.0))];
    let fx_log = RefCell::new(Vec::new());
    let mut fetch = mock_fetch(&prices);
    // 无任何日线（全段停牌 / 新上市不足一周）：历史缺 gracefully，现价照常更新。
    let mut kline = mock_kline(&[]);
    let mut fx = mock_fx(&[], &fx_log);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut kline, &mut fx, &mut no_nav).unwrap();

    assert_eq!(result.synced, 1, "无历史不中断同步");
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(100000));
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn kline_backfill_fetch_error_propagates() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    let prices = [("600519", Some(1000.0))];
    let fx_log = RefCell::new(Vec::new());
    let mut fetch = mock_fetch(&prices);
    let mut kline = |_: &str| Err(AppError::Io("模拟日 K 请求失败".into()));
    let mut fx = mock_fx(&[], &fx_log);
    let err =
        do_incremental_sync_with(&conn, &mut fetch, &mut kline, &mut fx, &mut no_nav).unwrap_err();
    assert!(err.to_string().contains("模拟日 K 请求失败"));
}

#[test]
fn fx_secid_candidates_cover_onshore_and_reverse_fallback() {
    // HKD→CNY：东财无 119.HKDCNY，主候选为在岸人民币市场 120.HKDCNYC；反向兜底取倒数。
    assert_eq!(
        fx_secid_candidates("HKDCNY"),
        vec![
            ("120.HKDCNYC".to_string(), false),
            ("119.HKDCNY".to_string(), false),
            ("119.CNYHKD".to_string(), true),
        ]
    );
    // 纯全球外汇对：119 直连 + 反向兜底。
    assert_eq!(
        fx_secid_candidates("EURUSD"),
        vec![
            ("119.EURUSD".to_string(), false),
            ("119.USDEUR".to_string(), true),
        ]
    );
    // 本位币为 base 的反向对：119 直连 + 119 反向 + 120 反向（取倒数）。
    assert_eq!(
        fx_secid_candidates("CNYHKD"),
        vec![
            ("119.CNYHKD".to_string(), false),
            ("119.HKDCNY".to_string(), true),
            ("120.HKDCNYC".to_string(), true),
        ]
    );
}

#[test]
fn kline_response_deserializes_daily_bars_and_skips_invalid() {
    // 真实 push2his 日 K 响应形状（fields2=f51,f53 → 每行 "日期,收盘价"）。
    let json = r#"{"rc":0,"rt":17,"svr":1,"lt":1,"full":0,"data":{"code":"600519","market":1,"name":"贵州茅台","decimal":2,"dktotal":566,"preKPrice":1302.8,"klines":["2026-01-05,1302.80","2026-01-06,1310.00","2026-01-07,-"]}}"#;
    let resp: KlineResponse = serde_json::from_str(json).unwrap();
    let data = resp.data.unwrap();
    let klines = data.klines.unwrap();
    let bars = parse_klines(&klines);
    assert_eq!(bars.len(), 2, "无效收盘样本（'-'）应被过滤");
    assert_eq!(bars[0].date, "2026-01-05");
    assert_eq!(bars[0].close, 1302.80);
    assert_eq!(bars[1].close, 1310.00);

    // 无效代码 / 无数据：data 为 null → 空序列，不报错（优雅降级）。
    let empty: KlineResponse = serde_json::from_str(r#"{"rc":100,"data":null}"#).unwrap();
    assert!(empty.data.is_none());
}

#[test]
fn week_key_matches_sqlite_week_start_column() {
    // Rust 侧降采样周键（week_monday）与 V010 week_start 生成列恒等——这是
    // 「整周覆盖幂等」的隐式契约：周键一旦漂移，ON CONFLICT 落点即错、产生重复周行。
    // 扫描跨年/闰年边界三年，每天与 SQLite 生成表达式比对。
    use crate::commands::sync::incremental::week_monday;
    use chrono::NaiveDate;

    let conn = setup_db();
    let mut d = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2027, 12, 31).unwrap();
    while d <= end {
        let iso = d.format("%Y-%m-%d").to_string();
        let rust_key = week_monday(d).format("%Y-%m-%d").to_string();
        let sql_key: String = conn
            .query_row(
                "SELECT date(?1, '-6 days', 'weekday 1')",
                params![iso],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rust_key, sql_key, "{iso} 的周键两侧不一致");
        d += chrono::Duration::days(1);
    }
}

// ---------------------------------------------------------------------------
// 基金分区：历史净值按水位增量回填（issue #303 / ADR-0038 决策 6）。编排经
// 注入 mock 页抓取闭包驱动，不依赖真实网络；水位语义（首刷近两年 / 增量从
// 水位次日起）与跨页降采样、同周整周覆盖在此端到端钉住。
// ---------------------------------------------------------------------------

/// 构造一页净值结果：total 为窗口内总条数（分页定界），points 为 (日期, 单位净值)。
fn nav_page(total: u64, points: &[(&str, f64)]) -> LsjzPage {
    LsjzPage {
        total,
        points: points
            .iter()
            .map(|(d, n)| NavPoint {
                date: d.to_string(),
                nav: *n,
            })
            .collect(),
    }
}

/// 模拟历史净值页抓取：按代码返回页序列（下标 = 页码 − 1，越界页返回空），
/// 并记录全部查询（断言水位窗口、翻页与「非可拉取行零请求」）。
fn mock_nav<'a>(
    pages_by_code: &'a [(&'a str, Vec<LsjzPage>)],
    requested: &'a RefCell<Vec<NavQuery>>,
) -> impl FnMut(&NavQuery) -> Result<LsjzPage> + 'a {
    move |query: &NavQuery| {
        requested.borrow_mut().push(query.clone());
        Ok(pages_by_code
            .iter()
            .find(|(c, _)| *c == query.code)
            .and_then(|(_, pages)| pages.get((query.page - 1) as usize))
            .cloned()
            .unwrap_or(LsjzPage {
                points: vec![],
                total: 0,
            }))
    }
}

/// 近两年首刷窗口起点（与 kline_beg / nav_window 同式，测试侧独立重算）。
fn expected_first_sync_start() -> String {
    beijing_today()
        .checked_sub_months(chrono::Months::new(24))
        .unwrap()
        .format("%Y-%m-%d")
        .to_string()
}

/// 基金现价缓存的 (price_cents, nav_date)（无行返回 None）。
fn fund_price_of(conn: &Connection, instrument_id: &str) -> Option<(i64, Option<String>)> {
    conn.query_row(
        "SELECT price_cents, nav_date FROM market_prices WHERE instrument_id=?1",
        params![instrument_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

#[test]
fn fund_first_sync_backfills_two_years_with_cross_page_weekly() {
    let conn = setup_db();
    insert_holding(
        &conn,
        "acc-1",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );

    // 首刷（无水位）：窗口 = 近两年。total=45 → 3 页；净值页按日期降序返回，
    // 第 1/2 页跨页同属 ISO 周（2026-01-26 起）——攒齐后一次降采样必须取该周
    // 最后一个净值日（01-30 周五），逐页落库会被后页的更早日期覆盖。
    let pages = [(
        "110022",
        vec![
            nav_page(45, &[("2026-01-30", 3.348), ("2026-01-28", 3.293)]),
            nav_page(45, &[("2026-01-26", 3.25), ("2025-12-31", 3.1)]),
            nav_page(45, &[]),
        ],
    )];
    let requested = RefCell::new(Vec::new());
    let mut fetch = mock_fetch(&[]);
    let mut nav = mock_nav(&pages, &requested);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut nav).unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.written, 1);

    // 翻页：首页起点 = 近两年窗口起点，共 3 页、全部同窗口。
    let requested = requested.borrow();
    assert_eq!(requested.len(), 3);
    for q in requested.iter() {
        assert_eq!(q.code, "110022");
        assert_eq!(q.start_date, expected_first_sync_start());
    }
    assert_eq!(requested[0].page, 1);
    assert_eq!(requested[1].page, 2);
    assert_eq!(requested[2].page, 3);

    // 周采样：跨页同周取最后净值日；单位净值 ×10000 得万分之一元（ADR-0038）。
    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![
            ("2025-12-31".into(), 31000, "CNY".into()),
            ("2026-01-30".into(), 33480, "CNY".into()),
        ],
    );

    // 现价 = 窗口内最新公布单位净值，priced_at = nav_date = 净值日期（下次水位）。
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((33480, Some("2026-01-30".into()))),
    );
}

#[test]
fn fund_incremental_fetches_from_watermark_and_overwrites_same_week() {
    let conn = setup_db();
    insert_holding(
        &conn,
        "acc-1",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );
    // 水位 = 现价缓存的净值日期 01-28（周三），上一轮已把该周采样写到周三。
    upsert_market_price(
        &conn,
        "inst-fund",
        30000,
        "CNY",
        "2026-01-28",
        Some("2026-01-28"),
        Some(EASTMONEY_PRICE_SOURCE),
    )
    .unwrap();
    upsert_price_history(
        &conn,
        "inst-fund",
        "2026-01-28",
        30000,
        "CNY",
        EASTMONEY_PRICE_SOURCE,
    )
    .unwrap();
    // 更早一周的历史点应原样保留（增量不回看）。
    upsert_price_history(
        &conn,
        "inst-fund",
        "2026-01-23",
        31000,
        "CNY",
        EASTMONEY_PRICE_SOURCE,
    )
    .unwrap();

    // 窗口 = 水位次日起，单页两行（total=2 → 1 页）：周四、周五新净值；
    // 周五与水位同周——该周采样整周覆盖为周五。
    let pages = [(
        "110022",
        vec![nav_page(2, &[("2026-01-30", 3.348), ("2026-01-29", 3.42)])],
    )];
    let requested = RefCell::new(Vec::new());
    let mut fetch = mock_fetch(&[]);
    let mut nav = mock_nav(&pages, &requested);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut nav).unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.written, 1);

    let requested = requested.borrow();
    assert_eq!(requested.len(), 1, "常态增量每只一页");
    assert_eq!(requested[0].start_date, "2026-01-29", "从水位次日起");

    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![
            ("2026-01-23".into(), 31000, "CNY".into()),
            ("2026-01-30".into(), 33480, "CNY".into()),
        ],
        "水位当日不重拉；同周新净值整周覆盖采样日"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((33480, Some("2026-01-30".into()))),
    );
}

#[test]
fn fund_incremental_up_to_date_counts_synced_without_write() {
    let conn = setup_db();
    insert_holding(
        &conn,
        "acc-1",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );
    // 水位较新（一周内），窗口内无新净值（mock 返回空页）。
    let watermark = beijing_today()
        .checked_sub_days(chrono::Days::new(7))
        .unwrap()
        .format("%Y-%m-%d")
        .to_string();
    upsert_market_price(
        &conn,
        "inst-fund",
        30000,
        "CNY",
        &watermark,
        Some(&watermark),
        Some(EASTMONEY_PRICE_SOURCE),
    )
    .unwrap();

    let requested = RefCell::new(Vec::new());
    let mut fetch = mock_fetch(&[]);
    let mut nav = mock_nav(&[], &requested);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut nav).unwrap();

    // 「已是最新」= 处理成功但不落库：synced 计入、written 为 0（零变化不广播）。
    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.written, 0);
    assert_eq!(result.message, "已同步 1 只，跳过 0 只");
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((30000, Some(watermark))),
        "无新净值不动现价"
    );
    assert_eq!(price_history_rows(&conn, "inst-fund"), vec![]);
}

#[test]
fn fund_first_sync_without_nav_counts_skipped() {
    let conn = setup_db();
    insert_holding(
        &conn,
        "acc-1",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );

    let mut fetch = mock_fetch(&[]);
    let mut nav = no_nav;
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut nav).unwrap();

    // 首刷查无净值（查无此码 / 新基金未公布首期）：计入跳过，不报错不落价。
    assert_eq!(result.synced, 0);
    assert_eq!(result.skipped, 1);
    assert_eq!(result.written, 0);
    assert_eq!(fund_price_of(&conn, "inst-fund"), None);
    assert_eq!(result.message, "已同步 0 只，跳过 1 只");
}

#[test]
fn fund_rows_without_real_code_skip_without_fetch() {
    let conn = setup_db();
    // 名称充代码的基金行（无真实代码，查不到净值）与债券：计入跳过、零请求。
    insert_holding(
        &conn,
        "acc-1",
        "inst-namefund",
        "华夏成长混合",
        "fund",
        "CNY",
        "unknown",
    );
    insert_holding(
        &conn,
        "acc-2",
        "inst-bond",
        "019547",
        "bond",
        "CNY",
        "unknown",
    );

    let requested = RefCell::new(Vec::new());
    let mut fetch = mock_fetch(&[]);
    let mut nav = mock_nav(&[], &requested);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut nav).unwrap();

    assert_eq!(result.synced, 0);
    assert_eq!(result.skipped, 2);
    assert_eq!(result.written, 0);
    assert!(requested.borrow().is_empty(), "不可拉取行不得发起净值请求");
}

#[test]
fn fund_nav_fetch_error_propagates() {
    let conn = setup_db();
    insert_holding(
        &conn,
        "acc-1",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );

    let mut fetch = mock_fetch(&[]);
    let mut nav = |_: &NavQuery| Err(AppError::Io("模拟净值请求失败".into()));
    let err = do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut nav)
        .unwrap_err();
    assert!(err.to_string().contains("模拟净值请求失败"));
}

#[test]
fn fund_and_stock_partitions_roll_up_into_one_result() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(
        &conn,
        "acc-2",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );
    insert_holding(
        &conn,
        "acc-3",
        "inst-namefund",
        "华夏成长混合",
        "fund",
        "CNY",
        "unknown",
    );
    insert_holding(
        &conn,
        "acc-4",
        "inst-bond",
        "019547",
        "bond",
        "CNY",
        "unknown",
    );

    let pages = [("110022", vec![nav_page(1, &[("2026-01-30", 3.348)])])];
    let requested = RefCell::new(Vec::new());
    let prices = [("600519", Some(130280.0))];
    let mut fetch = mock_fetch(&prices);
    let mut nav = mock_nav(&pages, &requested);
    let result =
        do_incremental_sync_with(&conn, &mut fetch, &mut no_kline, &mut no_fx, &mut nav).unwrap();

    // synced = 股票 1 + 基金 1；skipped = 名称充代码基金 + 债券；written = 2。
    assert_eq!(result.synced, 2);
    assert_eq!(result.skipped, 2);
    assert_eq!(result.written, 2);
    assert_eq!(result.message, "已同步 2 只，跳过 2 只");
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(13028000));
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((33480, Some("2026-01-30".into())))
    );
}
