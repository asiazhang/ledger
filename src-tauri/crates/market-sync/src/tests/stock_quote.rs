//! 股票按代码查询的取数接线（issue #693 / ADR-0130 决策 2 / issue #1567）：
//! 查询端点与创建增强的生产取数走腾讯行情批量报价端点（单只 = 一批一条）。本组
//! 用例把生产适配 [`crate::stock::fetch_stock_quote`] 对着本地 HTTP 服务驱动，
//! 钉住请求形态（路径段 `q=<市场前缀代码>`）、GBK 解码、统一报价载荷投影与查无
//! 此码语义——**删除腾讯接线（改回其他源或请求形状）即红**（负向接线证明，查询
//! 端点与创建增强的生产分支经源码扫描钉住直呼生产入口）。报文 fixture 为取数面
//! 实测（2026-09-19 采集）真实报文，与 `tests/tencent.rs` 共用同一份原文。

use std::time::Duration;

use crate::http::{Pacer, build_client};
use crate::stock::fetch_stock_quote;
use crate::tests::spawn_header_capture_server;
use crate::tests::tencent::{HK_STOCK, SH_ETF, SH_STOCK, US_AAPL};

/// 取请求头首行的请求目标（`GET <target> HTTP/1.1` 第二段）。
fn request_target(head: &str) -> &str {
    head.lines()
        .next()
        .unwrap_or("")
        .split(' ')
        .nth(1)
        .unwrap_or("")
}

/// 把 fixture 报文编码为 GBK 字节（腾讯端点为 GBK 通道，ADR-0130 决策 2）。
fn gbk(body: &str) -> Vec<u8> {
    encoding_rs::GBK.encode(body).0.into_owned()
}

// ---------------------------------------------------------------------------
// 接线证明：生产适配打到腾讯端点并投影统一报价（删除接线即红）
// ---------------------------------------------------------------------------

#[test]
fn fetch_pins_tencent_request_form_and_projects_quote() {
    let client = build_client().unwrap();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (url, requests) = spawn_header_capture_server(gbk(SH_STOCK));

    let quote = tauri::async_runtime::block_on(fetch_stock_quote(
        &client,
        &mut pacer,
        &[url.as_str()],
        "sh",
        "600000",
    ))
    .expect("假响应应命中");

    // 请求形态：路径段 q=<市场前缀代码>，无需 Referer（先例：批量报价通道钉住）。
    let captured = requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 1, "单只一次请求");
    assert_eq!(request_target(&captured[0]), "/q=sh600000");
    assert!(
        !captured[0].to_ascii_lowercase().contains("referer"),
        "腾讯端点无需 Referer"
    );

    // 统一报价载荷投影：代码 / 权威名称 / 价格（万分之一元）/ 行情日期 /
    // 精确市场 / 类型提示；场外成员恒缺省。
    assert_eq!(quote.code, "600000");
    assert_eq!(quote.name, "浦发银行", "应投影数据源权威名称");
    assert_eq!(
        quote.price_cents,
        Some(90_700),
        "9.07 元 → 万分之一元 90700"
    );
    assert_eq!(
        quote.price_date.as_deref(),
        Some("2026-09-18"),
        "应投影交易所当地交易日"
    );
    assert_eq!(quote.market.as_deref(), Some("sh"));
    assert_eq!(
        quote.stock_kind_hint(),
        ledger_investment::InstrumentType::Stock
    );
    assert_eq!(quote.fund_class, None, "场外成员恒缺省");
    assert_eq!(quote.nav_date, None);
    assert_eq!(quote.constant_unit_price_cents, None);
}

#[test]
fn fetch_projects_etf_kind_hint_from_tencent_type_code() {
    let client = build_client().unwrap();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (url, _) = spawn_header_capture_server(gbk(SH_ETF));

    let quote = tauri::async_runtime::block_on(fetch_stock_quote(
        &client,
        &mut pacer,
        &[url.as_str()],
        "sh",
        "510300",
    ))
    .expect("ETF 报文应命中");
    assert_eq!(
        quote.stock_kind_hint(),
        ledger_investment::InstrumentType::Etf,
        "场内基金类类型码应探测为 etf（stock/etf 同属行情通道）"
    );
    assert_eq!(quote.market.as_deref(), Some("sh"));
}

#[test]
fn us_ticker_single_query_maps_suffix_to_precise_market() {
    let client = build_client().unwrap();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (url, requests) = spawn_header_capture_server(gbk(US_AAPL));

    // 美股聚合路由值 us：一次查询，精确交易所由响应自报后缀判定
    //（三市场候选遍历退役，ADR-0130 决策 2 / issue #1567）。
    let quote = tauri::async_runtime::block_on(fetch_stock_quote(
        &client,
        &mut pacer,
        &[url.as_str()],
        "us",
        "AAPL",
    ))
    .expect("美股报文应命中");

    let captured = requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 1, "一次查询即命中，不必遍历三市场");
    assert_eq!(request_target(&captured[0]), "/q=usAAPL");
    assert_eq!(quote.code, "AAPL", "回显裸 ticker（去交易所后缀）");
    assert_eq!(
        quote.market.as_deref(),
        Some("nasdaq"),
        "自报 .OQ → 纳斯达克"
    );
    assert_eq!(quote.price_cents, Some(3_361_300), "336.13 美元");
    assert_eq!(
        quote.stock_kind_hint(),
        ledger_investment::InstrumentType::Stock
    );
}

#[test]
fn hk_query_uses_normalized_key_and_projects_hkd_name() {
    let client = build_client().unwrap();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (url, requests) = spawn_header_capture_server(gbk(HK_STOCK));

    let quote = tauri::async_runtime::block_on(fetch_stock_quote(
        &client,
        &mut pacer,
        &[url.as_str()],
        "hk",
        "00700",
    ))
    .expect("港股报文应命中");
    assert_eq!(request_target(&requests.lock().unwrap()[0]), "/q=hk00700");
    assert_eq!(quote.code, "00700", "回显 5 位补零形态");
    assert_eq!(quote.market.as_deref(), Some("hk"));
    assert_eq!(quote.name, "腾讯控股");
    assert_eq!(quote.price_cents, Some(4_190_000), "419.000 港元");
}

// ---------------------------------------------------------------------------
// 查无此码：码化拒绝，不静默退化（不建死行）
// ---------------------------------------------------------------------------

#[test]
fn all_invalid_batch_is_coded_not_found() {
    let client = build_client().unwrap();
    let mut pacer = Pacer::new(Duration::ZERO);
    // 数据源对批量内全部无效代码明示 pv_none_match：空命中 → 码化查无此码。
    let (url, _) = spawn_header_capture_server(gbk("v_pv_none_match=\"1\";"));

    let error = tauri::async_runtime::block_on(fetch_stock_quote(
        &client,
        &mut pacer,
        &[url.as_str()],
        "sh",
        "600000",
    ))
    .expect_err("全无效应报查无此码");
    assert!(
        error.is_code("sync.stock-not-found"),
        "查无此码应码化拒绝: {error:?}"
    );
    assert!(
        error.to_string().contains("查无股票代码 600000"),
        "应中文说明，实际: {error}"
    );
}

#[test]
fn echo_mismatch_is_miss_not_hit() {
    let client = build_client().unwrap();
    let mut pacer = Pacer::new(Duration::ZERO);
    // 回显代码与请求归一化代码不等：防错配判据（先例：东财 f57 回显全等）。
    let (url, _) = spawn_header_capture_server(gbk(SH_STOCK));

    let error = tauri::async_runtime::block_on(fetch_stock_quote(
        &client,
        &mut pacer,
        &[url.as_str()],
        "sh",
        "600519",
    ))
    .expect_err("回显不等应按查无此码");
    assert!(error.is_code("sync.stock-not-found"), "实际: {error:?}");
}

#[test]
fn unroutable_market_is_coded_internal_inconsistency() {
    let client = build_client().unwrap();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (url, requests) = spawn_header_capture_server(gbk(SH_STOCK));

    // 市场闭集漂移才落入：解析单点已限定沪深港 + 美股（含聚合 us）。
    let error = tauri::async_runtime::block_on(fetch_stock_quote(
        &client,
        &mut pacer,
        &[url.as_str()],
        "unknown",
        "600519",
    ))
    .expect_err("未知市场应拒绝");
    assert!(error.is_code("sync.secid-unroutable"), "实际: {error:?}");
    assert!(
        requests.lock().unwrap().is_empty(),
        "无法构造查询键不得发起网络请求"
    );
}
