//! 资金加权收益率（MoneyWeightedReturn）测试（ADR-0115 / issue #1195 / 词汇表
//! 「资金加权收益率」词条）。
//!
//! 两层各司其职：
//! - **求解器**（[`xirr`]，纯函数）：与手算 XIRR 样本逐值对齐（含尾差口径——
//!   预期值按定义方程 `Σ cf·(1+r)^(−t) = 0` 以实际天数/365 独立高精度解出），
//!   并覆盖无解不给数（无符号变化 / 全部同时点）与退化输入。
//! - **读投影**（[`query_money_weighted_return_summary_on`]，时钟注入）：覆盖
//!   验收清单的场景用例——单笔买入持有、定投、部分卖出、分红、转换（两腿）、
//!   缺价跳过、按币种分组、无解场景、DRIP 两腿自相抵、区间期初市值（含历史
//!   汇率折算）。

use chrono::NaiveDate;
use ledger_transaction::{
    SecurityOrigin, TransactionInput, create_transaction_internal, delete_transaction_internal,
};

use super::super::mwr::xirr;
use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{
    open, seed_account, seed_exchange_rate, seed_fx_history_weeks, seed_fx_rate_history,
    seed_instrument, seed_price_history,
};

/// 测试锚定「今天」：默认口径的期末现金流落在本日（2026-01-01 起整一年的
/// 样本设计，年化时间恰为 1.0）。
const TODAY: &str = "2027-01-01";

fn today() -> NaiveDate {
    NaiveDate::parse_from_str(TODAY, "%Y-%m-%d").unwrap()
}

/// 日期/手续费显式的买入输入（价格权威形态：金额 = 数量 × 单价 + 手续费）。
fn buy_on(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        date: date.into(),
        ..make_buy_input(account_id, instrument_id, qty, price, fee)
    }
}

/// 日期/手续费显式的卖出输入（净额 = 数量 × 单价 − 手续费）。
fn sell_on(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        date: date.into(),
        ..make_sell_input(account_id, instrument_id, qty, price, fee)
    }
}

/// 期初存量买入输入（issue #1343）：`origin = opening`，其余同 [`buy_on`]。
fn opening_buy_on(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        date: date.into(),
        origin: Some(SecurityOrigin::Opening),
        ..make_buy_input(account_id, instrument_id, qty, price, fee)
    }
}

/// 现金分红输入（金额权威，到账账户 = `account_id`）。
fn dividend_on(
    account_id: &str,
    instrument_id: &str,
    amount_cents: i64,
    date: &str,
) -> TransactionInput {
    let mut input = make_dividend_input(account_id, instrument_id, amount_cents, "CNY");
    input.date = date.into();
    input
}

fn mwr_on(conn: &rusqlite::Connection, range: &MwrRange) -> MoneyWeightedReturnSummary {
    mwr::query_money_weighted_return_summary_on(conn, range, today()).unwrap()
}

fn instrument_rate(
    summary: &MoneyWeightedReturnSummary,
    account_id: &str,
    instrument_id: &str,
) -> Option<f64> {
    summary
        .by_instrument
        .iter()
        .find(|r| r.account_id == account_id && r.instrument_id == instrument_id)
        .and_then(|r| r.rate)
}

fn account_rate(summary: &MoneyWeightedReturnSummary, account_id: &str) -> Option<f64> {
    summary
        .by_account
        .iter()
        .find(|r| r.account_id == account_id)
        .and_then(|r| r.rate)
}

fn total_rate(summary: &MoneyWeightedReturnSummary, currency: &str) -> Option<f64> {
    summary
        .total
        .iter()
        .find(|r| r.currency_code == currency)
        .and_then(|r| r.rate)
}

fn account_basis(summary: &MoneyWeightedReturnSummary, account_id: &str) -> Option<MwrBasis> {
    summary
        .by_account
        .iter()
        .find(|r| r.account_id == account_id)
        .map(|r| r.basis)
}

fn total_basis(summary: &MoneyWeightedReturnSummary, currency: &str) -> Option<MwrBasis> {
    summary
        .total
        .iter()
        .find(|r| r.currency_code == currency)
        .map(|r| r.basis)
}

/// 单标的行的口径（行缺失即 `None`）。
fn instrument_basis(
    summary: &MoneyWeightedReturnSummary,
    account_id: &str,
    instrument_id: &str,
) -> Option<MwrBasis> {
    summary
        .by_instrument
        .iter()
        .find(|r| r.account_id == account_id && r.instrument_id == instrument_id)
        .map(|r| r.basis)
}

fn assert_close(actual: Option<f64>, expected: f64) {
    let rate = actual.unwrap_or_else(|| panic!("预期有收益率，实际为 None（预期 {expected}）"));
    assert!(
        (rate - expected).abs() < 1e-9,
        "XIRR {rate} 与手算值 {expected} 偏差超过尾差口径 1e-9"
    );
}

// ---------------------------------------------------------------------------
// 求解器：手算样本逐值对齐 + 无解不给数
// ---------------------------------------------------------------------------

/// 手算样本矩阵行：[名称, 现金流 (年化时间, 金额), 手算解]。
type XirrCase = (&'static str, Vec<(f64, f64)>, f64);

#[test]
fn xirr_matches_hand_computed_samples() {
    // 矩阵：[名称, 现金流 (年化时间, 金额), 手算解]。手算解按定义方程
    // Σ cf·(1+r)^(−t) = 0 以实际天数/365 独立高精度解出（尾差口径 1e-9）。
    let cases: &[XirrCase] = &[
        // 单笔买入持有一年：10000 → 11000，年化恰 10%。
        ("单笔买入持有", vec![(0.0, -10000.0), (1.0, 11000.0)], 0.1),
        // 亏损年：10000 → 9000，年化 −10%。
        ("亏损年", vec![(0.0, -10000.0), (1.0, 9000.0)], -0.1),
        // 定投两笔（间隔 181 天）+ 期末市值。
        (
            "定投",
            vec![(0.0, -5000.0), (181.0 / 365.0, -5000.0), (1.0, 10600.0)],
            0.080_296_779_864_777_48,
        ),
        // 部分卖出（200 天回流 4000）+ 期末市值 7000。
        (
            "部分卖出",
            vec![(0.0, -10000.0), (200.0 / 365.0, 4000.0), (1.0, 7000.0)],
            0.121_236_342_832_381_08,
        ),
        // 持有期分红（100 天收 300）+ 期末市值 11200。
        (
            "分红",
            vec![(0.0, -10000.0), (100.0 / 365.0, 300.0), (1.0, 11200.0)],
            0.153_272_508_100_482_65,
        ),
        // 零金额流不入方程：与剔除后同解。
        (
            "零金额流",
            vec![(0.0, -10000.0), (0.5, 0.0), (1.0, 11000.0)],
            0.1,
        ),
    ];
    for (name, flows, expected) in cases {
        let rate = xirr(flows).unwrap_or_else(|| panic!("{name}: 预期有解 {expected}，实际 None"));
        assert!(
            (rate - expected).abs() < 1e-9,
            "{name}: XIRR {rate} 与手算值 {expected} 偏差超过尾差口径 1e-9"
        );
    }
}

#[test]
fn xirr_returns_none_for_unsolvable_or_degenerate_flows() {
    // 无解不给数（ADR-0115 代价 1）：
    // 仅一笔流（无符号变化）。
    assert_eq!(xirr(&[(0.0, -100.0)]), None);
    // 全为正流（无符号变化）。
    assert_eq!(xirr(&[(0.0, 100.0), (1.0, 50.0)]), None);
    // 全为负流。
    assert_eq!(xirr(&[(0.0, -100.0), (1.0, -50.0)]), None);
    // 空集。
    assert_eq!(xirr(&[]), None);
    // 全部同一时点（NPV 与 r 无关，无唯一解）。
    assert_eq!(xirr(&[(0.0, -100.0), (0.0, 100.0)]), None);
}

// ---------------------------------------------------------------------------
// 读投影：单笔买入持有 / 定投 / 部分卖出 / 分红
// ---------------------------------------------------------------------------

#[test]
fn mwr_single_buy_and_hold_matches_hand_computed_rate() {
    let conn = open();
    seed_account(&conn, "acc-m1", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-m1", "600519", "贵州茅台", "CNY", "unknown");
    // 买 10 份 @ 10 元（100000 刻度）→ 10000 分；现价 11 元 → 期末市值 11000 分。
    create_transaction_internal(
        &conn,
        buy_on("acc-m1", "inst-m1", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-m1", 110_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    // 手算：−10000 于 t0、+11000 于整一年 → r = 11000/10000 − 1 = 10%。
    assert_close(instrument_rate(&summary, "acc-m1", "inst-m1"), 0.1);
    assert_close(account_rate(&summary, "acc-m1"), 0.1);
    assert_close(total_rate(&summary, "CNY"), 0.1);
    assert_eq!(summary.by_instrument[0].currency_code, "CNY");
    // 非期初存量的标的仍是**年化**口径（#1343 不改变既有行为）；合集不含
    // 期初存量时账户级与全账级同规年化、逐位不变（#1346）。
    assert_eq!(summary.by_instrument[0].basis, MwrBasis::Annualized);
    assert_eq!(
        account_basis(&summary, "acc-m1"),
        Some(MwrBasis::Annualized)
    );
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Annualized));
}

#[test]
fn mwr_buy_fee_is_part_of_negative_flow() {
    // 买入为负（确认金额含手续费）：10000 分 + 手续费 100 分 → 流出 10100。
    let conn = open();
    seed_account(&conn, "acc-fee", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-fee", "600000", "浦发银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-fee", "inst-fee", 10.0, 100_000, 100, "2026-01-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-fee", 110_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    // 手算：−10100 → +11000 整一年 → r = 11000/10100 − 1。
    assert_close(account_rate(&summary, "acc-fee"), 11000.0 / 10100.0 - 1.0);
}

#[test]
fn mwr_dca_multiple_buys_matches_hand_computed_rate() {
    // 定投（多次买入）：−5000 与 −5000（间隔 181 天）→ 期末市值 10600。
    let conn = open();
    seed_account(&conn, "acc-dca", "定投户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-dca", "000001", "平安银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-dca", "inst-dca", 5.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-dca", "inst-dca", 5.0, 100_000, 0, "2026-07-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-dca", 106_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    // 手算样本：r = 0.08029677986477748（定义方程独立解出，尾差 1e-9）。
    assert_close(account_rate(&summary, "acc-dca"), 0.080_296_779_864_777_48);
}

#[test]
fn mwr_partial_sell_net_proceeds_are_positive_flow() {
    // 部分卖出：买 10000 分；200 天后卖 4 份（毛 4200 − 手续费 200 = 净 4000）；
    // 余 6 份期末市值 7000 分。
    let conn = open();
    seed_account(&conn, "acc-ps", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ps", "600036", "招商银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-ps", "inst-ps", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        sell_on("acc-ps", "inst-ps", 4.0, 105_000, 200, "2026-07-20"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-ps", 116_667, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    // 手算样本：−10000 / +4000(200d) / +7000(365d) → r = 0.12123634283238108。
    assert_close(account_rate(&summary, "acc-ps"), 0.121_236_342_832_381_08);
}

#[test]
fn mwr_dividend_is_positive_flow() {
    // 现金分红为正：买 10000 分；100 天后分红 300；期末市值 11200 分。
    let conn = open();
    seed_account(&conn, "acc-div", "基金户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-div", "000001", "华夏成长", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-div", "inst-div", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(&conn, dividend_on("acc-div", "inst-div", 300, "2026-04-11"))
        .unwrap();
    seed_market_price(&conn, "inst-div", 112_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    // 手算样本：−10000 / +300(100d) / +11200(365d) → r = 0.15327250810048265。
    assert_close(account_rate(&summary, "acc-div"), 0.153_272_508_100_482_65);
}

// ---------------------------------------------------------------------------
// 读投影：转换两腿 / split 零现金流
// ---------------------------------------------------------------------------

#[test]
fn mwr_convert_splits_into_two_legs_at_instrument_granularity() {
    // 基金转换（ADR-0099 / ADR-0115 决策 2）：单标的粒度按结转成本拆两腿
    // （转出 +、转入 −），账户级两腿抵净、不受转换影响。
    let conn = open();
    seed_account(&conn, "acc-cv", "转换户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-cv-a", "000001", "转出基金", "CNY", "unknown");
    seed_instrument(&conn, "inst-cv-b", "000002", "转入基金", "CNY", "unknown");
    // 买 A 10 份 @ 10 元（每份成本 100000 刻度）；100 天后转出 5 份换 B 5 份
    // （确认金额 4500，结转成本 = 5 × 100000 刻度 = 5000 分——行金额锚点）。
    create_transaction_internal(
        &conn,
        buy_on("acc-cv", "inst-cv-a", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            date: "2026-04-11".into(),
            ..make_convert_input("acc-cv", "inst-cv-a", "inst-cv-b", 5.0, 5.0, 4500, 4500, 0)
        },
    )
    .unwrap();
    // A、B 现价均 11 元 → 期末市值各 5500 分（各持 5 份）。
    seed_market_price(&conn, "inst-cv-a", 110_000, "CNY");
    seed_market_price(&conn, "inst-cv-b", 110_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    // 转出腿 A：−10000(0d) / +5000(100d，结转成本非确认金额 4500) / +5500(365d)。
    assert_close(
        instrument_rate(&summary, "acc-cv", "inst-cv-a"),
        0.078_034_331_116_894_84,
    );
    // 转入腿 B：−5000(100d，结转成本非确认金额 4500) / +5500(365d)。
    assert_close(
        instrument_rate(&summary, "acc-cv", "inst-cv-b"),
        0.140_282_781_268_959_96,
    );
    // 账户级与全账级不额外引入转换：两腿同日同额反号抵净 → 恰为
    // 「买 10000 → 期末 11000（5 份 A + 5 份 B）」的 10%。
    assert_close(account_rate(&summary, "acc-cv"), 0.1);
    assert_close(total_rate(&summary, "CNY"), 0.1);
}

#[test]
fn mwr_split_is_zero_cash_flow() {
    // 份额调整（split）零现金腿：3:1 折算后份额变多、现金流集不变——收益率
    // 只由期末市值（份额 × 现价）承载。
    let conn = open();
    seed_account(&conn, "acc-sp", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-sp", "600000", "浦发银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-sp", "inst-sp", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 20.0)).unwrap();
    // 折算后 30 份，现价不变 → 期末市值 30000 分。
    seed_market_price(&conn, "inst-sp", 100_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    // 现金流恰为「−10000 → +30000 整一年」（split 零贡献）→ r = 200%。
    assert_close(account_rate(&summary, "acc-sp"), 2.0);
}

// ---------------------------------------------------------------------------
// 读投影：缺价跳过 / 按币种分组 / 无解
// ---------------------------------------------------------------------------

#[test]
fn mwr_skips_unvalued_holding_from_row_and_aggregates() {
    // 缺价持仓（ADR-0115 决策 4）：该标的不给数，其现金流也不计入账户与全账
    // 合计——账户收益率恰等于「只有已估值标的」的口径。
    let conn = open();
    seed_account(&conn, "acc-np", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-np-1", "600519", "贵州茅台", "CNY", "unknown");
    seed_instrument(&conn, "inst-np-2", "000001", "平安银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-np", "inst-np-1", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-np", "inst-np-2", 5.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    // 只有 inst-np-1 有现价；inst-np-2 缺价。
    seed_market_price(&conn, "inst-np-1", 110_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    // 缺价行不给数（行缺席，前端渲染「-」）。
    assert_eq!(instrument_rate(&summary, "acc-np", "inst-np-2"), None);
    // 账户收益率只由已估值标的构成：恰为单标的买入持有口径 10%
    // （若缺价标的现金流被错误计入，流合集为 −15000/+11000，解必不同）。
    assert_close(account_rate(&summary, "acc-np"), 0.1);
    assert_close(total_rate(&summary, "CNY"), 0.1);
}

#[test]
fn mwr_groups_ledger_total_by_currency_without_mixing() {
    // 按币种分组（ADR-0107 同口径）：USD 与 CNY 账户各自成组、各自解年化。
    let conn = open();
    seed_account(&conn, "acc-us", "美股户", "investment", "USD", 0);
    seed_account(&conn, "acc-cn", "A 股户", "investment", "CNY", 0);
    // USD 买入写入按交易日取数（#1547）：2026-01-01 所属周（周一 2025-12-29）的历史点。
    seed_fx_history_weeks(&conn, "USD", "CNY", 1.0, &["2026-01-01"]);
    seed_instrument(&conn, "inst-us", "AAPL", "Apple", "USD", "unknown");
    seed_instrument(&conn, "inst-cn", "600519", "贵州茅台", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-us", "inst-us", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-cn", "inst-cn", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-us", 110_000, "USD");
    seed_market_price(&conn, "inst-cn", 120_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(summary.total.len(), 2);
    assert_close(total_rate(&summary, "USD"), 0.1);
    assert_close(total_rate(&summary, "CNY"), 0.2);
    assert_close(account_rate(&summary, "acc-us"), 0.1);
    assert_close(account_rate(&summary, "acc-cn"), 0.2);
}

#[test]
fn mwr_reports_none_when_flows_have_no_solution() {
    // 无解场景（ADR-0115 代价 1）：唯一现金流是一笔分红（无买入、无持仓、
    // 无期末市值）——现金流无符号变化，显式不给数而非猜解。
    let conn = open();
    seed_account(&conn, "acc-ns", "基金户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ns", "000001", "华夏成长", "CNY", "unknown");
    create_transaction_internal(&conn, dividend_on("acc-ns", "inst-ns", 300, "2026-04-11"))
        .unwrap();

    let summary = mwr_on(&conn, &MwrRange::default());
    // 无持仓 → 无单标的行；账户级有流但无解 → 行在、率为 None（无法计算）。
    assert_eq!(instrument_rate(&summary, "acc-ns", "inst-ns"), None);
    let account = summary
        .by_account
        .iter()
        .find(|r| r.account_id == "acc-ns")
        .expect("有现金流的账户应有账户级行");
    assert_eq!(account.rate, None);
    let total = summary
        .total
        .iter()
        .find(|r| r.currency_code == "CNY")
        .expect("全账级应有 CNY 组");
    assert_eq!(total.rate, None);
}

// ---------------------------------------------------------------------------
// 读投影：DRIP 两腿自相抵
// ---------------------------------------------------------------------------

#[test]
fn mwr_drip_legs_cancel_within_account() {
    // 红利再投（DRIP，#1288 / ADR-0109 修订）：dividend 到账投资账户 +500 与
    // 0 手续费 buy −500 同日同额反号，两腿在账户内自相抵、不构成外部投入——
    // 收益率恰等于「−10000 → 期末市值 10500（再投份额平价）」的手算口径。
    let conn = open();
    seed_account(&conn, "acc-dr", "基金户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-dr", "000001", "华夏成长", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-dr", "inst-dr", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(&conn, dividend_on("acc-dr", "inst-dr", 500, "2026-04-11"))
        .unwrap();
    // 再投买入：0 手续费、0.5 份 @ 平价 10 元（金额 500 分，自投资账户出资）。
    create_transaction_internal(
        &conn,
        buy_on("acc-dr", "inst-dr", 0.5, 100_000, 0, "2026-04-11"),
    )
    .unwrap();
    // 平价持有：现价不变 → 期末市值 = 10.5 份 × 100000 刻度 = 10500 分。
    seed_market_price(&conn, "inst-dr", 100_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    // 手算：DRIP 两腿同日抵净 → −10000(0d) / +10500(365d) → r = 5%。
    assert_close(account_rate(&summary, "acc-dr"), 0.05);
    assert_close(instrument_rate(&summary, "acc-dr", "inst-dr"), 0.05);
    assert_close(total_rate(&summary, "CNY"), 0.05);
}

// ---------------------------------------------------------------------------
// 读投影：区间选择（期初投入 = 区间首日市值，含历史汇率折算）
// ---------------------------------------------------------------------------

#[test]
fn mwr_range_folds_starting_position_at_start_date_market_value() {
    // 区间口径（ADR-0115 决策 3）：区间 [2026-07-01, 2027-01-01]，区间开始时
    // 存量 10 份按区间首日市值（10 × 11 元 = 11000 分）折为期初投入；区间末日
    // 市值 12000 分为期末现金流；区间前买入流水不重复入集。
    let conn = open();
    seed_account(&conn, "acc-rg", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-rg", "600519", "贵州茅台", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-rg", "inst-rg", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-rg-1", "inst-rg", "2026-01-05", 100_000, "CNY");
    seed_price_history(&conn, "ph-rg-2", "inst-rg", "2026-07-01", 110_000, "CNY");
    seed_price_history(&conn, "ph-rg-3", "inst-rg", "2026-12-25", 120_000, "CNY");

    let range = MwrRange {
        start_date: Some("2026-07-01".into()),
        end_date: Some(TODAY.into()),
    };
    let summary = mwr_on(&conn, &range);
    // 手算：−11000(184d 起点) / +12000(184d 后) → r = (12/11)^(365/184) − 1。
    assert_close(account_rate(&summary, "acc-rg"), 0.188_395_514_532_513_57);
    assert_close(
        instrument_rate(&summary, "acc-rg", "inst-rg"),
        0.188_395_514_532_513_57,
    );
}

#[test]
fn mwr_range_converts_boundary_value_with_period_fx_rate() {
    // 区间边界的历史折算纪律：CNY 账户持有 USD 标的，边界市值按价格行同期
    // 汇率（USD→CNY @ 7.0）折算，不用当期汇率近似（与组合走势同纪律）。
    let conn = open();
    seed_account(&conn, "acc-fx", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-fx", "AAPL", "Apple", "USD", "unknown");
    // 买入 10 份（交易行币种 = 账户币种 CNY，金额 10000 分）。
    create_transaction_internal(
        &conn,
        buy_on("acc-fx", "inst-fx", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    // USD 价格序列与同期汇率（同周键）。
    seed_price_history(&conn, "ph-fx-1", "inst-fx", "2026-07-01", 100_000, "USD");
    seed_fx_rate_history(&conn, "fxh-fx-1", "USD", "CNY", "2026-07-01", 7.0);
    seed_price_history(&conn, "ph-fx-2", "inst-fx", "2026-12-25", 110_000, "USD");
    seed_fx_rate_history(&conn, "fxh-fx-2", "USD", "CNY", "2026-12-25", 7.0);

    let range = MwrRange {
        start_date: Some("2026-07-01".into()),
        end_date: Some(TODAY.into()),
    };
    let summary = mwr_on(&conn, &range);
    // 期初市值 = 10 × 100000 刻度 ÷ 100 × 7.0 = 70000 分；期末 = 10 × 110000 ÷ 100 × 7.0 = 77000 分。
    // 手算：−70000 / +77000（184 天）→ r = (77/70)^(365/184) − 1。
    assert_close(account_rate(&summary, "acc-fx"), 0.208_121_156_121_199_33);
    assert_eq!(summary.total[0].currency_code, "CNY");
}

// ---------------------------------------------------------------------------
// 读投影：区间校验与软删口径
// ---------------------------------------------------------------------------

#[test]
fn mwr_range_includes_start_day_dividend() {
    // 范围外修复回归：分红是仓位外现金、不在期初市值折算内——起始日（含）的
    // 分红照常入集，只有起始日之前的窗口外流水排除。区间 [2026-07-01, 2027-01-01]：
    // 期初折算 −10000、当日分红 +500、期末现值 +10000（184d）→
    // r = (10000/9500)^(365/184) − 1（若起始日分红被折算吞掉，解应恰为 0）。
    let conn = open();
    seed_account(&conn, "acc-dd", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-dd", "600519", "贵州茅台", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-dd", "inst-dd", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(&conn, dividend_on("acc-dd", "inst-dd", 500, "2026-07-01"))
        .unwrap();
    seed_price_history(&conn, "ph-dd-1", "inst-dd", "2026-07-01", 100_000, "CNY");
    seed_market_price(&conn, "inst-dd", 100_000, "CNY");

    let range = MwrRange {
        start_date: Some("2026-07-01".into()),
        end_date: Some(TODAY.into()),
    };
    let summary = mwr_on(&conn, &range);
    assert_close(account_rate(&summary, "acc-dd"), 0.107_106_976_057_223_55);
}

#[test]
fn mwr_rejects_invalid_range() {
    let conn = open();
    let bad_format = MwrRange {
        start_date: Some("2026/07/01".into()),
        end_date: None,
    };
    let err = mwr::query_money_weighted_return_summary_on(&conn, &bad_format, today()).unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));

    seed_account(&conn, "acc-vd", "A 股户", "investment", "CNY", 0);
    let reversed = MwrRange {
        start_date: Some("2027-01-01".into()),
        end_date: Some("2026-07-01".into()),
    };
    let err = mwr::query_money_weighted_return_summary_on(&conn, &reversed, today()).unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
}

#[test]
fn mwr_excludes_soft_deleted_accounts_and_transactions() {
    // 软删口径（issue #217 定案）：软删账户的流水、软删交易行不入现金流集
    // （与 Holding / 已实现盈亏读口径对齐）。
    let conn = open();
    seed_account(&conn, "acc-sd", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-sd", "600519", "贵州茅台", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-sd", "inst-sd", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-sd", 110_000, "CNY");
    // 软删交易行（跌回成本价的买入），若未排除会把账户流拖成两笔买入。
    let dropped = create_transaction_internal(
        &conn,
        buy_on("acc-sd", "inst-sd", 5.0, 110_000, 0, "2026-06-01"),
    )
    .unwrap()
    .id;
    delete_transaction_internal(&conn, &dropped).unwrap();

    let summary = mwr_on(&conn, &MwrRange::default());
    // 软删买入已回补批次：恰为「买 10000 → 期末 11000（10 份 × 11 元）」→ 手算 r = 10%
    // （若软删流水未排除，流为 −15500、市值为 16500，解必不同）。
    assert_close(account_rate(&summary, "acc-sd"), 0.1);
}

// ---------------------------------------------------------------------------
// 期初存量（issue #1343 / #1345 / #1346 / ADR-0115 修订）：不计现金流；
// 仅存量无流水不给年化、单标的改给未年化，合集含仅存量对时整项降级（#1346）；
// 存量 + 真实流水年化恢复、起算点顺延到首笔真实流水，随普通对入年化合集（#1345）
// ---------------------------------------------------------------------------

#[test]
fn mwr_opening_balance_reports_cumulative_rate_instead_of_annualized() {
    // 补记存量（真实建仓时点未知）：成本 10000 分、现值 11000 分。
    // 年化不适用（不给数）；未年化 = 累计收益 ÷ 累计投入 = 1000 / 10000 = 10%。
    let conn = open();
    seed_account(&conn, "acc-op", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-op",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-op", "inst-op", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-op", 110_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(
        instrument_basis(&summary, "acc-op", "inst-op"),
        Some(MwrBasis::Cumulative),
        "含期初存量的标的应标未年化口径"
    );
    assert_close(instrument_rate(&summary, "acc-op", "inst-op"), 0.1);
    // 账户级 / 全账级不再整项缺席（issue #1346）：合集含期初存量 → 整项给
    // 未年化口径，分子分母各自汇总后相除（此处只有该标的，恰同单标的值）。
    assert_eq!(
        account_basis(&summary, "acc-op"),
        Some(MwrBasis::Cumulative)
    );
    assert_close(account_rate(&summary, "acc-op"), 0.1);
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Cumulative));
    assert_close(total_rate(&summary, "CNY"), 0.1);
}

#[test]
fn mwr_opening_balance_with_real_flows_restores_annualized_from_first_real_flow() {
    // 期初存量 10000（10 份 @ 10 元，2026-06-30 补记）+ 首笔真实流水 = 2026-09-01
    // 再买 5000（5 份 @ 10 元）→ 年化恢复：以首笔真实流水为起算点，期前存量按该日
    // 市值（时点持仓 15 份 × ≤ 该日最新周线 11 元 = 16500 分）折为期初投入；现值
    // 15 份 × 12 元 = 18000 分。手算：−16500(122d) / +18000 → r = (18/16.5)^(365/122) − 1。
    // （再买当日流水由折算承载不入集，若重复入集或折算漏含当日买入，解必不同。）
    let conn = open();
    seed_account(&conn, "acc-ob", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-ob",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-ob", "inst-ob", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-ob", "inst-ob", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-ob-1", "inst-ob", "2026-09-01", 110_000, "CNY");
    seed_market_price(&conn, "inst-ob", 120_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(
        instrument_basis(&summary, "acc-ob", "inst-ob"),
        Some(MwrBasis::Annualized),
        "含期初存量且有真实流水的标的应恢复年化口径"
    );
    assert_close(
        instrument_rate(&summary, "acc-ob", "inst-ob"),
        0.297_346_368_102_668_93,
    );
    // 恢复年化后现金流重新计入账户级与全账级合计（与该标的同解）。
    assert_close(account_rate(&summary, "acc-ob"), 0.297_346_368_102_668_93);
    assert_close(total_rate(&summary, "CNY"), 0.297_346_368_102_668_93);
}

#[test]
fn mwr_opening_balance_with_real_flows_ignores_range_selection() {
    // 含期初存量的对不受区间选择影响（#1343 纪律延续，issue #1345）：起算点恒为
    // 首笔真实流水、期末恒取现值，区间边界一律不施加——选了区间与不选同解。
    let conn = open();
    seed_account(&conn, "acc-or", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-or",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-or", "inst-or", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-or", "inst-or", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-or-1", "inst-or", "2026-09-01", 110_000, "CNY");
    seed_market_price(&conn, "inst-or", 120_000, "CNY");

    let range = MwrRange {
        start_date: Some("2026-10-01".into()),
        end_date: Some("2026-12-31".into()),
    };
    let summary = mwr_on(&conn, &range);
    assert_eq!(
        instrument_basis(&summary, "acc-or", "inst-or"),
        Some(MwrBasis::Annualized)
    );
    assert_close(
        instrument_rate(&summary, "acc-or", "inst-or"),
        0.297_346_368_102_668_93,
    );
}

#[test]
fn mwr_opening_balance_with_real_flows_skips_when_fold_price_missing() {
    // 期前存量折价缺历史价格（≤ 首笔真实流水日无周线点）→ 按既有空值语义整行
    // 跳过：行不给数、现金流不入账户级与全账级合计（不给数就不入合计）。
    let conn = open();
    seed_account(&conn, "acc-np", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-np",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-np", "inst-np", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-np", "inst-np", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    // 唯一周线点落在首笔真实流水之后：折算缺料。
    seed_price_history(&conn, "ph-np-1", "inst-np", "2026-10-01", 110_000, "CNY");
    seed_market_price(&conn, "inst-np", 120_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(instrument_rate(&summary, "acc-np", "inst-np"), None);
    assert_eq!(account_rate(&summary, "acc-np"), None);
    assert_eq!(total_rate(&summary, "CNY"), None);
}

#[test]
fn mwr_opening_balance_with_real_flows_folds_at_period_fx_rate() {
    // 判据 1 的「× 同期汇率」腿：USD 标的、CNY 账户，t0 折算按价格行同期汇率
    // （USD→CNY @ 7.0）折入，不用当期汇率近似——与区间期初市值同一历史折算
    // 纪律。折算 = 15 份 × 10 USD × 7.0 = 1050 元 = 105000 分；现值 = 15 × 11 ×
    // 7.0 = 115500 分。手算：r = (1155/1050)^(365/122) − 1。
    let conn = open();
    seed_account(&conn, "acc-of", "雪球基金", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-of", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-of", "inst-of", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-of", "inst-of", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-of-1", "inst-of", "2026-09-01", 100_000, "USD");
    seed_fx_rate_history(&conn, "fxh-of-1", "USD", "CNY", "2026-09-01", 7.0);
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    seed_market_price(&conn, "inst-of", 110_000, "USD");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(
        instrument_basis(&summary, "acc-of", "inst-of"),
        Some(MwrBasis::Annualized)
    );
    assert_close(
        instrument_rate(&summary, "acc-of", "inst-of"),
        0.329_960_587_626_393_8,
    );
}

#[test]
fn mwr_opening_balance_with_real_flows_skips_when_fold_fx_missing() {
    // 判据 3 的「缺历史汇率」腿：t0 折算缺同期汇率（现价与当期汇率俱在，唯
    // 历史汇率缺）→ 按既有空值语义整行跳过、不入合计。
    let conn = open();
    seed_account(&conn, "acc-nf", "雪球基金", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-nf", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-nf", "inst-nf", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-nf", "inst-nf", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-nf-1", "inst-nf", "2026-09-01", 100_000, "USD");
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    seed_market_price(&conn, "inst-nf", 110_000, "USD");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(instrument_rate(&summary, "acc-nf", "inst-nf"), None);
    assert_eq!(account_rate(&summary, "acc-nf"), None);
    assert_eq!(total_rate(&summary, "CNY"), None);
}

#[test]
fn mwr_mixed_account_merges_restored_flows_with_regular_pairs() {
    // 同账户混布普通标的与「存量 + 真实流水」标的：恢复后的现金流并入账户级
    // 合计（若仍按含期初存量整对排除，账户解应恰为普通标的的 10%）。
    // 账户流：−10000(0d) −16500(243d) +11000+18000(365d)，手算解 16.4057%。
    let conn = open();
    seed_account(&conn, "acc-mx", "雪球基金", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-mr", "600519", "贵州茅台", "CNY", "unknown");
    seed_instrument(
        &conn,
        "inst-mo",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    // 普通标的：买 10000 分 → 现值 11000 分（年化 10%）。
    create_transaction_internal(
        &conn,
        buy_on("acc-mx", "inst-mr", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-mr", 110_000, "CNY");
    // 存量 + 真实流水标的：折算 −16500（2026-09-01）→ 现值 18000 分。
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-mx", "inst-mo", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-mx", "inst-mo", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-mo-1", "inst-mo", "2026-09-01", 110_000, "CNY");
    seed_market_price(&conn, "inst-mo", 120_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    // 两个单标的行各自口径不互相污染（普通标的逐位不变，issue #1345 验收 4）。
    assert_close(instrument_rate(&summary, "acc-mx", "inst-mr"), 0.1);
    assert_close(
        instrument_rate(&summary, "acc-mx", "inst-mo"),
        0.297_346_368_102_668_93,
    );
    // 账户级与全账级并入恢复后的现金流。
    assert_close(account_rate(&summary, "acc-mx"), 0.164_056_601_667_387_7);
    assert_close(total_rate(&summary, "CNY"), 0.164_056_601_667_387_7);
}

#[test]
fn mwr_opening_balance_ignores_range_boundaries() {
    // 仅期初存量标的的未年化口径是**生命周期**度量：选了区间也不改口径、不折期初
    // 市值（区间边界一律不施加），结果与不设区间逐位相同。
    let conn = open();
    seed_account(&conn, "acc-or", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-or",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-or", "inst-or", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-or", 110_000, "CNY");

    let range = MwrRange {
        start_date: Some("2026-09-01".into()),
        end_date: Some("2026-12-31".into()),
    };
    let summary = mwr_on(&conn, &range);
    assert_eq!(
        instrument_basis(&summary, "acc-or", "inst-or"),
        Some(MwrBasis::Cumulative)
    );
    assert_close(instrument_rate(&summary, "acc-or", "inst-or"), 0.1);
}

#[test]
fn mwr_origin_is_rejected_on_non_buy_kinds() {
    // 准入守卫（issue #1343）：`origin` 只描述证券扩展行的来源，只有 buy 有该扩展行。
    let conn = open();
    seed_account(&conn, "acc-og", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-og", "600519", "贵州茅台", "CNY", "unknown");
    let mut input = sell_on("acc-og", "inst-og", 1.0, 100_000, 0, "2026-01-01");
    input.origin = Some(SecurityOrigin::Opening);

    let error = create_transaction_internal(&conn, input).unwrap_err();
    assert!(
        error.to_string().contains("不能携带证券来源口径"),
        "卖出携带 origin 应被准入守卫拒绝，实际: {error}"
    );
}

// ---------------------------------------------------------------------------
// 合集口径（issue #1346 / ADR-0115 修订）：账户级与全账级在合集含期初存量时
// 整项降级为未年化——分子（累计收益）分母（累计投入）对全合集各自汇总后相除
// ---------------------------------------------------------------------------

#[test]
fn mwr_collection_with_opening_only_pair_degrades_despite_restored_pair() {
    // 两票交互（#1345 × #1346）：同账户既有仅存量对（仍不给年化）又有
    // 「存量 + 真实流水」对（年化已恢复）——合集仍整项降级为未年化（仅存量
    // 对触发，issue #1346），且降级 Σ 按成本帧计：t0 折算（市值锚）不进分子
    // 分母。对 A：收益 1000 ÷ 投入 10000；对 B：收益 (18000 − 5000) − 10000 =
    // 3000 ÷ 投入 15000 → Σ 4000 ÷ Σ 25000 = 16%（若折算 −15000 误入成本帧，
    // Σ 收益变负，断言即红）。
    let conn = open();
    seed_account(&conn, "acc-ix", "雪球基金", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ix-a", "003155", "仅存量标的", "CNY", "unknown");
    seed_instrument(
        &conn,
        "inst-ix-b",
        "003156",
        "存量再买标的",
        "CNY",
        "unknown",
    );
    // 对 A：仅存量（10000 → 现值 11000）。
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-ix", "inst-ix-a", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-ix-a", 110_000, "CNY");
    // 对 B：存量 + 真实流水（折算 15000 = 15 份 × 10 元，期末 18000 = 15 × 12）。
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-ix", "inst-ix-b", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-ix", "inst-ix-b", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-ix-1", "inst-ix-b", "2026-09-01", 100_000, "CNY");
    seed_market_price(&conn, "inst-ix-b", 120_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    // 单标的行各自口径：仅存量未年化、存量 + 真实流水年化恢复。
    assert_eq!(
        instrument_basis(&summary, "acc-ix", "inst-ix-a"),
        Some(MwrBasis::Cumulative)
    );
    assert_eq!(
        instrument_basis(&summary, "acc-ix", "inst-ix-b"),
        Some(MwrBasis::Annualized)
    );
    // 合集整项降级，Σ 按成本帧（折算不进分子分母）。
    assert_eq!(
        account_basis(&summary, "acc-ix"),
        Some(MwrBasis::Cumulative)
    );
    assert_close(account_rate(&summary, "acc-ix"), 0.16);
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Cumulative));
    assert_close(total_rate(&summary, "CNY"), 0.16);
}

#[test]
fn mwr_mixed_collection_degrades_whole_item_to_cumulative() {
    // 混合场景（issue #1346 定案：整项降级，不按口径拆两行）：同账户既有
    // 期初存量（投入 10000、现值 11000，收益 1000）又有真实成交标的
    // （投入 5000、现值 5400，收益 400）——账户级与全账级给未年化：
    // Σ收益 1400 ÷ Σ投入 15000 = 9.33…%（不是两个口径两行，也不是只算真实成交）。
    let conn = open();
    seed_account(&conn, "acc-mx", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-mx-op",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    seed_instrument(&conn, "inst-mx-tr", "000001", "平安银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-mx", "inst-mx-op", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-mx", "inst-mx-tr", 5.0, 100_000, 0, "2026-08-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-mx-op", 110_000, "CNY");
    seed_market_price(&conn, "inst-mx-tr", 108_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    // 单标的行各自口径不变：期初存量未年化、真实成交仍年化。
    assert_eq!(
        instrument_basis(&summary, "acc-mx", "inst-mx-op"),
        Some(MwrBasis::Cumulative)
    );
    assert_eq!(
        instrument_basis(&summary, "acc-mx", "inst-mx-tr"),
        Some(MwrBasis::Annualized)
    );
    // 合集整项降级：Σ收益 (1000 + 400) ÷ Σ投入 (10000 + 5000)。
    assert_eq!(
        account_basis(&summary, "acc-mx"),
        Some(MwrBasis::Cumulative)
    );
    assert_close(account_rate(&summary, "acc-mx"), 1400.0 / 15000.0);
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Cumulative));
    assert_close(total_rate(&summary, "CNY"), 1400.0 / 15000.0);
}

#[test]
fn mwr_currency_groups_flip_basis_independently() {
    // 口径按币种分组各自裁决（#1346）：CNY 组含期初存量 → 未年化；USD 组
    // 全为真实成交 → 年化逐位不变。
    let conn = open();
    seed_account(&conn, "acc-oc", "雪球基金", "investment", "CNY", 0);
    seed_account(&conn, "acc-uc", "美股户", "investment", "USD", 0);
    // USD 买入写入按交易日取数（#1547）：2026-01-01 所属周的历史点。
    seed_fx_history_weeks(&conn, "USD", "CNY", 1.0, &["2026-01-01"]);
    seed_instrument(
        &conn,
        "inst-oc",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    seed_instrument(&conn, "inst-uc", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-oc", "inst-oc", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-uc", "inst-uc", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-oc", 110_000, "CNY");
    seed_market_price(&conn, "inst-uc", 120_000, "USD");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(
        account_basis(&summary, "acc-oc"),
        Some(MwrBasis::Cumulative)
    );
    assert_eq!(
        account_basis(&summary, "acc-uc"),
        Some(MwrBasis::Annualized)
    );
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Cumulative));
    assert_eq!(total_basis(&summary, "USD"), Some(MwrBasis::Annualized));
    assert_close(account_rate(&summary, "acc-oc"), 0.1);
    assert_close(account_rate(&summary, "acc-uc"), 0.2);
    assert_close(total_rate(&summary, "CNY"), 0.1);
    assert_close(total_rate(&summary, "USD"), 0.2);
}

#[test]
fn mwr_mixed_collection_range_keeps_cumulative_with_lifetime_opening() {
    // 区间不改变合集口径（#1346）：含期初存量的合集仍整项给未年化；其中
    // 期初存量腿是生命周期度量（区间边界不施加），区间前买入的真实成交腿
    // 不重复入集。期初存量（投入 10000、现值 11000、收益 1000）+ 真实成交
    // （投入 5000、现值 5000、收益 0）→ Σ收益 1000 ÷ Σ投入 15000。
    let conn = open();
    seed_account(&conn, "acc-mr", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-mr-op",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    seed_instrument(&conn, "inst-mr-tr", "000001", "平安银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-mr", "inst-mr-op", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-mr", "inst-mr-tr", 5.0, 100_000, 0, "2026-08-01"),
    )
    .unwrap();
    // 区间首日（2026-09-01）前的周线价：期初市值 = 5 × 10 元 = 5000 分（缺口料
    // 会让该对整体跳过，而非入集）。
    seed_price_history(&conn, "ph-mr-1", "inst-mr-tr", "2026-08-15", 100_000, "CNY");
    seed_market_price(&conn, "inst-mr-op", 110_000, "CNY");
    seed_market_price(&conn, "inst-mr-tr", 100_000, "CNY");

    let range = MwrRange {
        start_date: Some("2026-09-01".into()),
        end_date: Some(TODAY.into()),
    };
    let summary = mwr_on(&conn, &range);
    assert_eq!(
        account_basis(&summary, "acc-mr"),
        Some(MwrBasis::Cumulative)
    );
    assert_close(account_rate(&summary, "acc-mr"), 1000.0 / 15000.0);
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Cumulative));
    assert_close(total_rate(&summary, "CNY"), 1000.0 / 15000.0);
}

#[test]
fn mwr_degraded_collection_excludes_unvalued_pairs_from_aggregate() {
    // 降级合集的空值语义不变（#1346）：缺价 / 缺汇率的对整对跳过，不入未年化
    // 的 Σ分子 / Σ分母——期初存量（10000 → 11000）+ 真实成交（5000 → 6000）
    // + 缺价成交（投入 8000）：合计应为 2000/15000；若缺价对被错误计入则是
    // 2000/23000。
    let conn = open();
    seed_account(&conn, "acc-du", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-du-op",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    seed_instrument(&conn, "inst-du-tr", "000001", "平安银行", "CNY", "unknown");
    seed_instrument(&conn, "inst-du-np", "600000", "浦发银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-du", "inst-du-op", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-du", "inst-du-tr", 5.0, 100_000, 0, "2026-08-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-du", "inst-du-np", 8.0, 100_000, 0, "2026-08-01"),
    )
    .unwrap();
    // 只有前两个标的有现价；inst-du-np 缺价。
    seed_market_price(&conn, "inst-du-op", 110_000, "CNY");
    seed_market_price(&conn, "inst-du-tr", 120_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(
        account_basis(&summary, "acc-du"),
        Some(MwrBasis::Cumulative)
    );
    assert_close(account_rate(&summary, "acc-du"), 2000.0 / 15000.0);
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Cumulative));
    assert_close(total_rate(&summary, "CNY"), 2000.0 / 15000.0);
}

// ---------------------------------------------------------------------------
// 恒定价格标的的边界市值（ADR-0126 决策 6 / issue #1450）：收益率边界市值与
// 组合走势共用「≤ 截止日最新周线」的取价纪律，恒定标的在响应内按常量取值——
// 「删除即变红」：删掉 AsOfValues 的常量覆盖，下方用例吃到存量平坦行的错值。
// ---------------------------------------------------------------------------

#[test]
fn mwr_boundary_value_uses_constant_price_not_flat_history_rows() {
    let conn = open();
    seed_account(&conn, "acc-const", "货基户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-const", "000198", "天弘余额宝");
    conn.execute(
        "UPDATE instruments SET constant_unit_price = 10000 WHERE id = 'inst-const'",
        [],
    )
    .unwrap();
    // 确认单金额权威：100 份 × 1.0000 = 100 元（单价由金额与份额反算，不可携带）。
    let mut buy = buy_on("acc-const", "inst-const", 100.0, 0, 0, "2026-01-01");
    buy.amount_cents = 10_000;
    buy.price_cents = None;
    create_transaction_internal(&conn, buy).unwrap();
    // 起始日分红 +10 元（期初折算不含它，起始日分红入集）。
    create_transaction_internal(
        &conn,
        dividend_on("acc-const", "inst-const", 1_000, "2026-07-01"),
    )
    .unwrap();
    // 存量平坦序列（值刻意偏离常量）：读侧消费它即红。
    seed_price_history(&conn, "ph-flat", "inst-const", "2026-07-01", 20_000, "CNY");

    let range = MwrRange {
        start_date: Some("2026-07-01".into()),
        end_date: Some(TODAY.into()),
    };
    let summary = mwr_on(&conn, &range);
    // 边界市值 = 100 份 × 1.0000 = 10000 分（平坦行 20000 分不被消费）：
    // −10000(07-01 期初) / +1000(当日分红，t=0 折入分母) / +10000(期末，184d)
    // → r = (10000/9000)^(365/184) − 1。
    assert_close(
        account_rate(&summary, "acc-const"),
        0.232_448_938_442_890_44,
    );
    assert_close(
        instrument_rate(&summary, "acc-const", "inst-const"),
        0.232_448_938_442_890_44,
    );
}
