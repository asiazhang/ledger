use std::time::Duration;

use crate::http::{Pacer, build_client};
use ledger_investment::{Market, StockRoute};

use crate::stock::fetch_stock_quote;
use crate::tests::spawn_header_capture_server;
use crate::tests::tencent::{HK_STOCK, SH_ETF, SH_STOCK, US_AAPL};

fn request_target(head: &str) -> &str {
    head.lines()
        .next()
        .unwrap_or("")
        .split(' ')
        .nth(1)
        .unwrap_or("")
}

fn gbk(body: &str) -> Vec<u8> {
    encoding_rs::GBK.encode(body).0.into_owned()
}

#[test]
fn fetch_pins_tencent_request_form_and_projects_quote() {
    let client = build_client().unwrap();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (url, requests) = spawn_header_capture_server(gbk(SH_STOCK));

    let quote = tauri::async_runtime::block_on(fetch_stock_quote(
        &client,
        &mut pacer,
        &[url.as_str()],
        StockRoute::Sh,
        "600000",
    ))
    .expect("假响应应命中");

    let captured = requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 1, "单只一次请求");
    assert_eq!(request_target(&captured[0]), "/q=sh600000");
    assert!(
        !captured[0].to_ascii_lowercase().contains("referer"),
        "腾讯端点无需 Referer"
    );

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
    assert_eq!(quote.market, Some(Market::Sh));
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
        StockRoute::Sh,
        "510300",
    ))
    .expect("ETF 报文应命中");
    assert_eq!(
        quote.stock_kind_hint(),
        ledger_investment::InstrumentType::Etf,
        "场内基金类类型码应探测为 etf（stock/etf 同属行情通道）"
    );
    assert_eq!(quote.market, Some(Market::Sh));
}

#[test]
fn us_ticker_single_query_maps_suffix_to_precise_market() {
    let client = build_client().unwrap();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (url, requests) = spawn_header_capture_server(gbk(US_AAPL));

    let quote = tauri::async_runtime::block_on(fetch_stock_quote(
        &client,
        &mut pacer,
        &[url.as_str()],
        StockRoute::Us,
        "AAPL",
    ))
    .expect("美股报文应命中");

    let captured = requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 1, "一次查询即命中，不必遍历三市场");
    assert_eq!(request_target(&captured[0]), "/q=usAAPL");
    assert_eq!(quote.code, "AAPL", "回显裸 ticker（去交易所后缀）");
    assert_eq!(quote.market, Some(Market::Nasdaq), "自报 .OQ → 纳斯达克");
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
        StockRoute::Hk,
        "00700",
    ))
    .expect("港股报文应命中");
    assert_eq!(request_target(&requests.lock().unwrap()[0]), "/q=hk00700");
    assert_eq!(quote.code, "00700", "回显 5 位补零形态");
    assert_eq!(quote.market, Some(Market::Hk));
    assert_eq!(quote.name, "腾讯控股");
    assert_eq!(quote.price_cents, Some(4_190_000), "419.000 港元");
}

#[test]
fn all_invalid_batch_is_coded_not_found() {
    let client = build_client().unwrap();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (url, _) = spawn_header_capture_server(gbk("v_pv_none_match=\"1\";"));

    let error = tauri::async_runtime::block_on(fetch_stock_quote(
        &client,
        &mut pacer,
        &[url.as_str()],
        StockRoute::Sh,
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
    let (url, _) = spawn_header_capture_server(gbk(SH_STOCK));

    let error = tauri::async_runtime::block_on(fetch_stock_quote(
        &client,
        &mut pacer,
        &[url.as_str()],
        StockRoute::Sh,
        "600519",
    ))
    .expect_err("回显不等应按查无此码");
    assert!(error.is_code("sync.stock-not-found"), "实际: {error:?}");
}
