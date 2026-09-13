//! 东财基金搜索报文解析与命中挑选（issue #301 / ADR-0103）：fixture 驱动，不依赖
//! 真实网络。夹具取自 FundSearchAPI.ashx 真实响应（截取形态保持字段拼读不变）；
//! 投影为行情接入统一报价载荷（价格已在访问层换算为万分之一元刻度）。

use std::time::Duration;

use crate::sync::fund::{FundSearchResponse, fetch_fund_quote_from, pick_fund_quote};
use crate::sync::http::Pacer;

/// 真实响应形态：同一关键词命中基金（FundBaseInfo 非空）与股票（null）混排。
const MIXED_RESPONSE: &str = r#"{
  "ErrCode": 0,
  "ErrMsg": "fromcache",
  "Datas": [
    {
      "CODE": "000001", "NAME": "华夏成长混合", "JP": "HXCZHH",
      "CATEGORY": 700, "CATEGORYDESC": "基金",
      "FundBaseInfo": {
        "_id": "000001", "DWJZ": 1.318, "FCODE": "000001", "FSRQ": "2026-08-28",
        "FTYPE": "混合型-灵活", "SHORTNAME": "华夏成长混合",
        "JJGS": "华夏基金", "FUNDTYPE": "002"
      }
    },
    {
      "CODE": "000001", "NAME": "平安银行", "CATEGORY": 150, "CATEGORYDESC": "深市",
      "FundBaseInfo": null
    },
    {
      "CODE": "000001", "NAME": "上证指数", "CATEGORY": 600, "CATEGORYDESC": "指数",
      "FundBaseInfo": null
    }
  ]
}"#;

fn parse(raw: &str) -> FundSearchResponse {
    serde_json::from_str(raw).expect("fixture 应为合法 JSON")
}

#[test]
fn picks_fund_among_mixed_category_hits() {
    // 基金 / 股票 / 指数混排：只命中带 FundBaseInfo 且 FCODE 全等的基金条目。
    let resp = parse(MIXED_RESPONSE);
    let quote = pick_fund_quote(&resp, "000001").expect("应命中基金条目");
    assert_eq!(quote.code, "000001");
    assert_eq!(quote.name, "华夏成长混合");
    assert_eq!(quote.fund_class.as_deref(), Some("混合型-灵活"));
    // 净值 1.3180 元 → 13180 万分之一元（ADR-0038 价格刻度）；价格日期即净值日期。
    assert_eq!(quote.price_cents, Some(13_180));
    assert_eq!(quote.nav_date.as_deref(), Some("2026-08-28"));
    assert_eq!(quote.price_date.as_deref(), Some("2026-08-28"));
}

#[test]
fn no_fund_entry_means_not_found() {
    // 查无基金（仅股票/指数条目，或空 Datas）→ None（上层转中文「查无基金代码」）。
    let stock_only = r#"{"ErrCode":0,"Datas":[
        {"CODE":"600519","NAME":"贵州茅台","CATEGORY":100,"FundBaseInfo":null}
    ]}"#;
    assert!(pick_fund_quote(&parse(stock_only), "600519").is_none());

    let empty = r#"{"ErrCode":0,"Datas":[]}"#;
    assert!(pick_fund_quote(&parse(empty), "000001").is_none());

    // Datas 缺省（接口异常形态）同样按无命中处理。
    let missing = r#"{"ErrCode":0}"#;
    assert!(pick_fund_quote(&parse(missing), "000001").is_none());
}

#[test]
fn fund_code_must_match_exactly() {
    // 名称凑巧含代码的其他基金条目不得命中：FCode 全等是唯一判定。
    let other_fund = r#"{"Datas":[
        {"NAME":"别的基金","FundBaseInfo":{"FCODE":"000002","SHORTNAME":"别的基金",
         "FTYPE":"股票型","DWJZ":2.0,"FSRQ":"2026-08-28"}}
    ]}"#;
    assert!(pick_fund_quote(&parse(other_fund), "000001").is_none());
}

#[test]
fn nav_as_numeric_string_is_accepted() {
    // DWJZ 的 wire 形态数字 / 数字字符串都出现过，兼容解析。
    let raw = r#"{"Datas":[
        {"FundBaseInfo":{"FCODE":"510300","SHORTNAME":"沪深300ETF联接",
         "FTYPE":"指数型-股票","DWJZ":"1.2345","FSRQ":"2026-08-27"}}
    ]}"#;
    let quote = pick_fund_quote(&parse(raw), "510300").expect("应命中");
    assert_eq!(
        quote.price_cents,
        Some(12_345),
        "净值应为数字字符串形态可解析（1.2345 元 → 12345 万分之一元）"
    );
    assert_eq!(quote.nav_date.as_deref(), Some("2026-08-27"));
}

#[test]
fn missing_nav_pair_yields_none_nav() {
    // 新发基金尚未公布净值：DWJZ / FSRQ 任一缺省或非正值 → nav None（仍返回详情，
    // 上层据此仅建标的、不落现价、不广播价格失效信号）。
    let no_nav = r#"{"Datas":[
        {"FundBaseInfo":{"FCODE":"012345","SHORTNAME":"新发基金",
         "FTYPE":"混合型","DWJZ":null,"FSRQ":null}}
    ]}"#;
    let quote = pick_fund_quote(&parse(no_nav), "012345").expect("应命中");
    assert!(quote.price_cents.is_none());

    let no_date = r#"{"Datas":[
        {"FundBaseInfo":{"FCODE":"012345","SHORTNAME":"新发基金",
         "FTYPE":"混合型","DWJZ":1.0}}
    ]}"#;
    assert!(
        pick_fund_quote(&parse(no_date), "012345")
            .expect("应命中")
            .price_cents
            .is_none()
    );

    let zero_nav = r#"{"Datas":[
        {"FundBaseInfo":{"FCODE":"012345","SHORTNAME":"新发基金",
         "FTYPE":"混合型","DWJZ":0,"FSRQ":"2026-08-28"}}
    ]}"#;
    assert!(
        pick_fund_quote(&parse(zero_nav), "012345")
            .expect("应命中")
            .price_cents
            .is_none()
    );
}

#[test]
fn name_falls_back_to_item_name() {
    // SHORTNAME 缺省时回退条目外层 NAME。
    let raw = r#"{"Datas":[
        {"NAME":"外层名称基金","FundBaseInfo":{"FCODE":"000003",
         "FTYPE":"债券型-长债","DWJZ":1.01,"FSRQ":"2026-08-28"}}
    ]}"#;
    let quote = pick_fund_quote(&parse(raw), "000003").expect("应命中");
    assert_eq!(quote.name, "外层名称基金");
}

// ---------------------------------------------------------------------------
// 搜索索引未命中 → 档案通道回退（ADR-0039 修订，issue #1212）
// 本地 HTTP 服务同一端口分派两个通道的路径，验证回退接线与投影。
// ---------------------------------------------------------------------------

/// 搜索索引只收在用基金：已终止（清盘）代码命中的是别的类别条目（本例为同码股票）。
const SEARCH_INDEX_MISS: &str = r#"{"ErrCode":0,"ErrMsg":"fromcache","Datas":[
    {"CODE":"002503","NAME":"某股票","CATEGORY":150,"CATEGORYDESC":"深市","FundBaseInfo":null}
]}"#;

/// 档案通道（基金详情页数据文件）真实形态截取：名称 / 代码变量 + 单位净值序列
/// （已终止基金 002503 的末两点，末点即最后一期净值）。
const ARCHIVE_JS: &str = r#"/*2023-11-19 00:21:59*/var ishb=false;var fS_name = "中银腾利混合C";var fS_code = "002503";
var Data_netWorthTrend = [{"x":1694448000000,"y":1.138,"equityReturn":-0.09,"unitMoney":""},{"x":1694966400000,"y":1.144,"equityReturn":0.62,"unitMoney":""}];
var Data_ACWorthTrend = [[1694966400000,1.415]];"#;

/// 无效代码被重定向到的错误页形态（`pingzhongdata` 对 999999 的实测结果）。
const NOT_FOUND_PAGE: &str =
    r#"<html><head><title>页面未找到 - 东方财富网</title></head><body></body></html>"#;

/// 起一个本地 HTTP 服务：按请求路径把搜索通道与档案通道分派到两份响应体。
fn spawn_channel_server(search_body: &'static str, archive_body: &'static str) -> String {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let head = String::from_utf8_lossy(&buf).to_string();
            let body = if head.contains("FundSearchAPI") {
                search_body
            } else {
                archive_body
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    url
}

#[test]
fn quote_falls_back_to_archive_channel_when_search_index_misses() {
    // 删除 fetch_fund_quote_from 里的档案回退调用 → 本用例变红（接线型负向判据）。
    let url = spawn_channel_server(SEARCH_INDEX_MISS, ARCHIVE_JS);
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);

    let quote = fetch_fund_quote_from(
        &client,
        &mut pacer,
        "002503",
        &[url.as_str()],
        &[url.as_str()],
    )
    .expect("搜索索引未命中应回退档案通道命中");

    assert_eq!(quote.code, "002503");
    assert_eq!(quote.name, "中银腾利混合C", "名称取自档案通道");
    // 1.1440 元 → 11440 万分之一元；价格日期即最后一期净值日期。
    assert_eq!(quote.price_cents, Some(11_440));
    assert_eq!(quote.nav_date.as_deref(), Some("2023-09-18"));
    assert_eq!(quote.price_date.as_deref(), Some("2023-09-18"));
    assert_eq!(quote.fund_class, None, "档案通道无基金分类成员");
}

#[test]
fn quote_reports_not_found_when_archive_channel_also_misses() {
    // 两段皆未命中（档案文件是错误页）才是查无此码。
    let url = spawn_channel_server(SEARCH_INDEX_MISS, NOT_FOUND_PAGE);
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);

    let error = fetch_fund_quote_from(
        &client,
        &mut pacer,
        "002503",
        &[url.as_str()],
        &[url.as_str()],
    )
    .expect_err("档案通道也未命中应报查无此码");

    assert!(
        error.is_code("sync.fund-not-found"),
        "错误码应保持 sync.fund-not-found: {error:?}"
    );
}
