use ledger_transaction::amount::TransactionKind;
use ledger_transaction::create_transaction_internal;
use rusqlite::{Connection, params};

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{open, seed_account, seed_fx_history_weeks, seed_instrument};

// ---------------------------------------------------------------------------
// 时点持仓（AsOfHolding，spec #168 / issue #218）：
// 仅认 buy/sell 流水 · sell 取负 · 按交易日（含当日）前缀求和。
// ---------------------------------------------------------------------------

#[test]
fn holdings_as_of_sums_multiple_buys_and_sells_within_same_week() {
    let conn = open();
    seed_account(&conn, "acc-ao", "证券户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ao", "000001", "平安银行", "CNY", "unknown");
    // 同一周内三笔流水：买 10 → 卖 4 → 买 2（周采样键不影响推算，前缀只认交易日）。
    for (kind, qty, price, date) in [
        (TransactionKind::Buy, 10.0, 1500, "2026-02-04"),
        (TransactionKind::Sell, 4.0, 1600, "2026-02-05"),
        (TransactionKind::Buy, 2.0, 1500, "2026-02-06"),
    ] {
        create_transaction_internal(
            &conn,
            make_trade_input(kind, "acc-ao", "inst-ao", qty, price, date),
        )
        .unwrap();
    }

    // 前缀含当日：02-04 → 10；周末 02-08 → 10−4+2 = 8。
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao"), "2026-02-04").unwrap();
    assert!((qty - 10.0).abs() < 1e-9);
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao"), "2026-02-08").unwrap();
    assert!((qty - 8.0).abs() < 1e-9);

    // as_of 键为交易日（ISO YYYY-MM-DD），格式非法时显式报错（时间键契约显式化）。
    let err = holdings::holdings_as_of(&conn, Some("inst-ao"), "2026/02/08").unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
}

#[test]
fn holdings_as_of_replays_flow_before_query_range_start() {
    // 区间起点前买入（历史时点回放）：前缀求和把查询区间之前的流水累积带入。
    let conn = open();
    seed_account(&conn, "acc-ao2", "证券户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ao2", "600519", "贵州茅台", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-ao2",
            "inst-ao2",
            10.0,
            1500,
            "2026-01-05",
        ),
    )
    .unwrap();

    // 查询时点（02-01）晚于买入一个多月：数量仍为 10。
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao2"), "2026-02-01").unwrap();
    assert!((qty - 10.0).abs() < 1e-9);
    // 早于全部流水的时点：数量 0。
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao2"), "2025-12-31").unwrap();
    assert!((qty - 0.0).abs() < 1e-9);
}

#[test]
fn holdings_as_of_supports_historical_points_after_full_exit() {
    // 已清仓后查历史时点：清仓时点前有持仓、清仓时点后归零。
    let conn = open();
    seed_account(&conn, "acc-ao3", "证券户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ao3", "000001", "平安银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-ao3",
            "inst-ao3",
            10.0,
            1500,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-ao3",
            "inst-ao3",
            10.0,
            1600,
            "2026-01-20",
        ),
    )
    .unwrap();

    let qty = holdings::holdings_as_of(&conn, Some("inst-ao3"), "2026-01-15").unwrap();
    assert!((qty - 10.0).abs() < 1e-9);
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao3"), "2026-01-25").unwrap();
    assert!((qty - 0.0).abs() < 1e-9);
}

#[test]
fn holdings_as_of_is_currency_agnostic_for_cross_currency_instrument() {
    // 跨币种标的：模块只管数量，币种与折算不进推算。
    let conn = open();
    seed_account(&conn, "acc-usd", "美股户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-usd", "AAPL", "苹果", "USD", "unknown");
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    ); // 买卖落库经 Amount 接缝需要当期汇率
    create_transaction_internal(&conn, make_buy_input("acc-usd", "inst-usd", 5.0, 10_000, 0))
        .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-usd", "inst-usd", 2.0, 11_000, 0),
    )
    .unwrap();

    let qty = holdings::holdings_as_of(&conn, Some("inst-usd"), "2026-01-31").unwrap();
    assert!((qty - 3.0).abs() < 1e-9);
}

#[test]
fn holdings_as_of_without_instrument_sums_whole_portfolio() {
    // 全组合形态（instrument_id=None）：所有标的数量之和，与单标的形态同接缝。
    let conn = open();
    seed_account(&conn, "acc-ao4", "证券户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-a", "000001", "平安银行", "CNY", "unknown");
    seed_instrument(&conn, "inst-b", "600519", "贵州茅台", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-ao4",
            "inst-a",
            3.0,
            1500,
            "2026-02-04",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-ao4",
            "inst-b",
            7.0,
            1500,
            "2026-02-05",
        ),
    )
    .unwrap();

    let qty = holdings::holdings_as_of(&conn, None, "2026-02-06").unwrap();
    assert!((qty - 10.0).abs() < 1e-9);
    let qty_a = holdings::holdings_as_of(&conn, Some("inst-a"), "2026-02-06").unwrap();
    assert!((qty_a - 3.0).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// 首笔持仓流水日（issue #1534）：四臂腿流（buy/sell、convert 两腿、split）的
// 最早交易日——价格历史回填深度的覆盖目标。与 as-of / 腿流同一推算不变量的
// MIN 投影：认同一组腿、排除同一组软删行/户；dividend 零份额变动不入判。
// ---------------------------------------------------------------------------

#[test]
fn first_position_date_takes_earliest_leg_date() {
    let conn = open();
    seed_account(&conn, "acc-fp", "证券户", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-fp",
        "000025",
        "大摩双利增强债券C",
        "CNY",
        "unknown",
    );
    for (kind, qty, price, date) in [
        (TransactionKind::Buy, 10.0, 1500, "2020-06-10"),
        (TransactionKind::Buy, 5.0, 1600, "2021-03-04"),
        (TransactionKind::Sell, 3.0, 1700, "2022-08-18"),
    ] {
        create_transaction_internal(
            &conn,
            make_trade_input(kind, "acc-fp", "inst-fp", qty, price, date),
        )
        .unwrap();
    }

    let first = holdings::first_position_date(&conn, "inst-fp").unwrap();
    assert_eq!(first.as_deref(), Some("2020-06-10"), "首笔腿 = 最早交易日");
}

#[test]
fn first_position_date_counts_convert_legs_and_split() {
    // 转入标的（to_instrument_id）经 convert 转入腿建仓：其首笔腿 = 转换日。
    let conn = open();
    seed_account(&conn, "acc-fp2", "证券户", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-fp-out",
        "000024.OF",
        "转出基金",
        "CNY",
        "unknown",
    );
    seed_instrument(
        &conn,
        "inst-fp-in",
        "000025.OF",
        "转入基金",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-fp2",
            "inst-fp-out",
            10.0,
            1500,
            "2025-12-01",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_convert_input(
            "acc-fp2",
            "inst-fp-out",
            "inst-fp-in",
            10.0,
            12.0,
            15_000,
            15_000,
            0,
        ),
    )
    .unwrap();

    let out = holdings::first_position_date(&conn, "inst-fp-out").unwrap();
    let inp = holdings::first_position_date(&conn, "inst-fp-in").unwrap();
    assert_eq!(out.as_deref(), Some("2025-12-01"), "转出标的 = 前置买入日");
    assert_eq!(
        inp.as_deref(),
        Some("2026-02-01"),
        "转入标的 = 转换日（转入腿命中）"
    );
}

#[test]
fn first_position_date_excludes_dividend_and_soft_deleted_rows() {
    let conn = open();
    seed_account(&conn, "acc-fp3", "证券户", "investment", "CNY", 0);
    seed_account(&conn, "acc-fp3-gone", "待删户", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-fp-div",
        "000026",
        "仅分红标的",
        "CNY",
        "unknown",
    );
    seed_instrument(
        &conn,
        "inst-fp-gone",
        "000027",
        "软删户标的",
        "CNY",
        "unknown",
    );
    seed_instrument(
        &conn,
        "inst-fp-del",
        "000028",
        "软删流水标的",
        "CNY",
        "unknown",
    );
    // dividend 不改变持有数量：仅分红标的无首笔腿。
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-fp3", "inst-fp-div", 1_000, "CNY", "2026-01-05"),
    )
    .unwrap();
    // 软删账户名下的买入不进判（与腿流/as-of 同过滤）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-fp3-gone",
            "inst-fp-gone",
            10.0,
            1500,
            "2026-01-06",
        ),
    )
    .unwrap();
    // 软删流水的买入不进判；同标的在场买入更晚。
    let mut deleted_buy = make_trade_input(
        TransactionKind::Buy,
        "acc-fp3",
        "inst-fp-del",
        2.0,
        1500,
        "2019-01-07",
    );
    deleted_buy.note = Some("fp-to-delete".into());
    create_transaction_internal(&conn, deleted_buy).unwrap();
    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE note='fp-to-delete'",
        [],
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-fp3",
            "inst-fp-del",
            8.0,
            1500,
            "2026-01-08",
        ),
    )
    .unwrap();

    assert_eq!(
        holdings::first_position_date(&conn, "inst-fp-div").unwrap(),
        None,
        "仅分红（零份额变动）标的无首笔持仓腿"
    );
    conn.execute(
        "UPDATE accounts SET is_deleted=1 WHERE id='acc-fp3-gone'",
        [],
    )
    .unwrap();
    assert_eq!(
        holdings::first_position_date(&conn, "inst-fp-gone").unwrap(),
        None,
        "软删账户的买入不进判"
    );
    assert_eq!(
        holdings::first_position_date(&conn, "inst-fp-del")
            .unwrap()
            .as_deref(),
        Some("2026-01-08"),
        "软删流水不进判，取在场最早腿"
    );
}

#[test]
fn first_position_date_none_for_instrument_without_legs() {
    let conn = open();
    seed_instrument(
        &conn,
        "inst-fp-none",
        "000029",
        "无流水标的",
        "CNY",
        "unknown",
    );
    assert_eq!(
        holdings::first_position_date(&conn, "inst-fp-none").unwrap(),
        None,
        "无持仓流水 → None（覆盖目标维持近两年）"
    );
}

/// 绑定不变式（spec #168 定案第 6 条 / issue #218）：同一批 buy/sell 流水下，
/// as-of「今天」≡ Holding 数量口径（v_holdings 聚合，即 lots remaining_quantity 之和）。
/// 未来 split 落地改变数量时最先报警的哨兵。
///
/// 本模块口径已排除软删除账户（issue #217 定案，与 v_holdings 一致），不变式对
/// 软删夹具同样成立；含软删账户的组合走势断言（#219 接线后走势经本接缝同口径）
/// 归 issue #247 单测、#248 e2e，夹具此处从简不涉软删。
#[test]
fn holdings_as_of_today_matches_holding_quantity() {
    let conn = open();
    seed_account(&conn, "acc-ao5", "证券户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ao5", "000001", "平安银行", "CNY", "unknown");
    for (kind, qty, price, date) in [
        (TransactionKind::Buy, 10.0, 1500, "2026-01-05"),
        (TransactionKind::Buy, 5.0, 1600, "2026-01-12"),
        (TransactionKind::Sell, 8.0, 1700, "2026-01-18"),
    ] {
        create_transaction_internal(
            &conn,
            make_trade_input(kind, "acc-ao5", "inst-ao5", qty, price, date),
        )
        .unwrap();
    }

    // as-of「今天」（取晚于全部流水的交易日）= v_holdings 数量 = 10+5−8 = 7。
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao5"), "2026-06-01").unwrap();
    let holding_qty: Option<f64> = conn
        .query_row(
            "SELECT SUM(quantity) FROM v_holdings WHERE instrument_id=?1",
            params!["inst-ao5"],
            |r| r.get(0),
        )
        .unwrap();
    assert!((qty - 7.0).abs() < 1e-9, "as-of = {qty}");
    assert!(
        holding_qty.is_some_and(|q| (q - qty).abs() < 1e-9),
        "Holding = {holding_qty:?}"
    );

    // 清仓后：v_holdings 无行，as-of「今天」归零——两侧仍然一致。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-ao5",
            "inst-ao5",
            7.0,
            1700,
            "2026-01-20",
        ),
    )
    .unwrap();
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao5"), "2026-06-01").unwrap();
    let holding_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM v_holdings WHERE instrument_id=?1",
            params!["inst-ao5"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(holding_rows, 0);
    assert!((qty - 0.0).abs() < 1e-9, "清仓后 as-of = {qty}");
}

// ---------------------------------------------------------------------------
// 腿流投影 ≡ as-of 配对测试（issue #1654）：`holdings_legs_by_instrument` 与
// `holdings_as_of` 是同一推算不变量的两种形态（流水前缀投影 / 逐时点聚合）。
// 两处 SQL 的同步纪律由本测试钉住——任何一侧口径变化（腿类型、软删过滤、
// NULL 条件）而另一侧未同步，前缀和与 as-of 即在混合夹具上分叉变红。
// ---------------------------------------------------------------------------

#[test]
fn holdings_legs_stream_prefix_sums_match_as_of_on_mixed_fixture() {
    let conn = open();
    seed_account(&conn, "acc-lg", "证券户", "investment", "CNY", 0);
    seed_account(&conn, "acc-lg-gone", "待删户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-lg-a", "000001", "平安银行", "CNY", "unknown");
    seed_instrument(&conn, "inst-lg-b", "600036", "招商银行", "CNY", "unknown");
    seed_instrument(
        &conn,
        "inst-lg-out",
        "000001.OF",
        "转出基金",
        "CNY",
        "unknown",
    );
    seed_instrument(
        &conn,
        "inst-lg-in",
        "000002.OF",
        "转入基金",
        "CNY",
        "unknown",
    );
    seed_instrument(
        &conn,
        "inst-lg-none",
        "000008",
        "无腿标的",
        "CNY",
        "unknown",
    );

    // inst-a：同标的买/卖/买多腿 + split 带符号 Δ + 一笔待软删的买入。
    for (kind, qty, price, date) in [
        (TransactionKind::Buy, 10.0, 1500, "2026-02-04"),
        (TransactionKind::Sell, 4.0, 1600, "2026-02-05"),
        (TransactionKind::Buy, 2.0, 1500, "2026-02-06"),
    ] {
        create_transaction_internal(
            &conn,
            make_trade_input(kind, "acc-lg", "inst-lg-a", qty, price, date),
        )
        .unwrap();
    }
    let mut deleted_buy = make_trade_input(
        TransactionKind::Buy,
        "acc-lg",
        "inst-lg-a",
        2.0,
        1500,
        "2026-02-07",
    );
    deleted_buy.note = Some("legs-to-delete".into());
    create_transaction_internal(&conn, deleted_buy).unwrap();
    let mut split = make_split_input("acc-lg", "inst-lg-a", 3.0);
    split.date = "2026-02-08".into();
    create_transaction_internal(&conn, split).unwrap();

    // inst-b：挂在另一账户（后面整户软删）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-lg-gone",
            "inst-lg-b",
            5.0,
            1500,
            "2026-02-04",
        ),
    )
    .unwrap();

    // convert 的前置批次：转出基金先买入 6 份（转换按 FIFO 消耗在用批次）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-lg",
            "inst-lg-out",
            6.0,
            1500,
            "2026-02-03",
        ),
    )
    .unwrap();

    // convert 一笔两腿：转出腿 −6、转入腿 +12（两标的各得一条腿）。
    let mut convert = make_convert_input(
        "acc-lg",
        "inst-lg-out",
        "inst-lg-in",
        6.0,
        12.0,
        10_000,
        10_000,
        0,
    );
    convert.date = "2026-02-10".into();
    create_transaction_internal(&conn, convert).unwrap();

    let check_pairing = |conn: &Connection, label: &str| {
        let stream = holdings::holdings_legs_by_instrument(conn).unwrap();
        assert!(
            !stream.contains_key("inst-lg-none"),
            "无腿标的不进腿流（{label}）"
        );
        for (instrument_id, legs) in &stream {
            let dates: Vec<&str> = legs.iter().map(|(d, _)| d.as_str()).collect();
            let mut sorted = dates.clone();
            sorted.sort_unstable();
            assert_eq!(
                dates, sorted,
                "{instrument_id} 腿流应按交易日升序（{label}）"
            );
            let mut running = 0.0f64;
            for (date, qty) in legs {
                running += qty;
                let as_of = holdings::holdings_as_of(conn, Some(instrument_id), date).unwrap();
                assert!(
                    (running - as_of).abs() < 1e-9,
                    "{label}：{instrument_id} @{date} 腿流前缀和 {running} ≠ as-of {as_of}"
                );
            }
            // 第三投影绑定（issue #1534）：首笔持仓流水日 = 腿流首行日期。
            let first = holdings::first_position_date(conn, instrument_id).unwrap();
            assert_eq!(
                first.as_deref(),
                Some(legs[0].0.as_str()),
                "{label}：{instrument_id} 首笔腿 ≠ 腿流首行"
            );
            assert!(
                !holdings::first_position_date(conn, "inst-lg-none")
                    .unwrap()
                    .is_some(),
                "{label}：无腿标的无首笔持仓腿"
            );
        }
        stream
    };

    // 基线：四臂腿（buy/sell 取负、convert 两腿、split Δ）与过滤（软删行/户）
    // 全部在场，两形态逐标的逐前缀一致。
    let stream = check_pairing(&conn, "基线");
    assert_eq!(
        stream["inst-lg-a"].len(),
        5,
        "买 10 + 卖 −4 + 买 2 + 待删买 2 + split Δ3"
    );
    assert_eq!(stream["inst-lg-b"].len(), 1);
    assert_eq!(stream["inst-lg-out"].len(), 2, "前置买入 +6 与转出腿 −6");
    assert_eq!(stream["inst-lg-in"].len(), 1, "转入腿 +12");
    assert_eq!(
        stream["inst-lg-in"][0].1, 12.0,
        "convert 转入腿取 to_quantity"
    );

    // 软删一笔交易（无公开软删入口，库内状态直置——本文件既有软删用例同款
    // 显式例外）：腿流与 as-of 同步剔除该腿，配对保持。
    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE note='legs-to-delete'",
        [],
    )
    .unwrap();
    let stream = check_pairing(&conn, "软删一笔买入后");
    assert_eq!(stream["inst-lg-a"].len(), 4, "软删买入的腿不再投影");

    // 软删整户（同款显式例外）：inst-b 整标的不再有腿，配对保持。
    conn.execute(
        "UPDATE accounts SET is_deleted=1 WHERE id='acc-lg-gone'",
        [],
    )
    .unwrap();
    let stream = check_pairing(&conn, "软删账户后");
    assert!(!stream.contains_key("inst-lg-b"), "软删账户的腿流整体消失");
    let qty = holdings::holdings_as_of(&conn, Some("inst-lg-b"), "2026-02-06").unwrap();
    assert!((qty - 0.0).abs() < 1e-9, "as-of 同步排除软删账户");
}
