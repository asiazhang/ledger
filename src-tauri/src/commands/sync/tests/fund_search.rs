//! 东财基金搜索报文解析与命中挑选（issue #301）：fixture 驱动，不依赖真实网络。
//! 夹具取自 FundSearchAPI.ashx 真实响应（截取形态保持字段拼读不变）。

use crate::commands::sync::fund::{FundSearchResponse, pick_fund_detail};

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
    let detail = pick_fund_detail(&resp, "000001").expect("应命中基金条目");
    assert_eq!(detail.code, "000001");
    assert_eq!(detail.name, "华夏成长混合");
    assert_eq!(detail.fund_class, "混合型-灵活");
    let nav = detail.nav.expect("应带最新净值");
    assert!((nav.nav - 1.318).abs() < f64::EPSILON);
    assert_eq!(nav.nav_date, "2026-08-28");
}

#[test]
fn no_fund_entry_means_not_found() {
    // 查无基金（仅股票/指数条目，或空 Datas）→ None（上层转中文「查无基金代码」）。
    let stock_only = r#"{"ErrCode":0,"Datas":[
        {"CODE":"600519","NAME":"贵州茅台","CATEGORY":100,"FundBaseInfo":null}
    ]}"#;
    assert!(pick_fund_detail(&parse(stock_only), "600519").is_none());

    let empty = r#"{"ErrCode":0,"Datas":[]}"#;
    assert!(pick_fund_detail(&parse(empty), "000001").is_none());

    // Datas 缺省（接口异常形态）同样按无命中处理。
    let missing = r#"{"ErrCode":0}"#;
    assert!(pick_fund_detail(&parse(missing), "000001").is_none());
}

#[test]
fn fund_code_must_match_exactly() {
    // 名称凑巧含代码的其他基金条目不得命中：FCode 全等是唯一判定。
    let other_fund = r#"{"Datas":[
        {"NAME":"别的基金","FundBaseInfo":{"FCODE":"000002","SHORTNAME":"别的基金",
         "FTYPE":"股票型","DWJZ":2.0,"FSRQ":"2026-08-28"}}
    ]}"#;
    assert!(pick_fund_detail(&parse(other_fund), "000001").is_none());
}

#[test]
fn nav_as_numeric_string_is_accepted() {
    // DWJZ 的 wire 形态数字 / 数字字符串都出现过，兼容解析。
    let raw = r#"{"Datas":[
        {"FundBaseInfo":{"FCODE":"510300","SHORTNAME":"沪深300ETF联接",
         "FTYPE":"指数型-股票","DWJZ":"1.2345","FSRQ":"2026-08-27"}}
    ]}"#;
    let nav = pick_fund_detail(&parse(raw), "510300")
        .expect("应命中")
        .nav
        .expect("净值应为数字字符串形态可解析");
    assert!((nav.nav - 1.2345).abs() < f64::EPSILON);
    assert_eq!(nav.nav_date, "2026-08-27");
}

#[test]
fn missing_nav_pair_yields_none_nav() {
    // 新发基金尚未公布净值：DWJZ / FSRQ 任一缺省或非正值 → nav None（仍返回详情，
    // 上层据此仅建标的、不落现价、不广播价格失效信号）。
    let no_nav = r#"{"Datas":[
        {"FundBaseInfo":{"FCODE":"012345","SHORTNAME":"新发基金",
         "FTYPE":"混合型","DWJZ":null,"FSRQ":null}}
    ]}"#;
    let detail = pick_fund_detail(&parse(no_nav), "012345").expect("应命中");
    assert!(detail.nav.is_none());

    let no_date = r#"{"Datas":[
        {"FundBaseInfo":{"FCODE":"012345","SHORTNAME":"新发基金",
         "FTYPE":"混合型","DWJZ":1.0}}
    ]}"#;
    assert!(
        pick_fund_detail(&parse(no_date), "012345")
            .expect("应命中")
            .nav
            .is_none()
    );

    let zero_nav = r#"{"Datas":[
        {"FundBaseInfo":{"FCODE":"012345","SHORTNAME":"新发基金",
         "FTYPE":"混合型","DWJZ":0,"FSRQ":"2026-08-28"}}
    ]}"#;
    assert!(
        pick_fund_detail(&parse(zero_nav), "012345")
            .expect("应命中")
            .nav
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
    let detail = pick_fund_detail(&parse(raw), "000003").expect("应命中");
    assert_eq!(detail.name, "外层名称基金");
}
