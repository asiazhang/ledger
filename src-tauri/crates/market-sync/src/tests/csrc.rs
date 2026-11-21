use std::time::Duration;

use super::spawn_capture_server;
use crate::csrc::{
    CsrcNavRecord, confirm_money_fund_form_from, fetch_fund_nav_series_from, parse_disclosure_page,
};

const NORMAL_FUND_PAYLOAD: &str = r#"{"sEcho":1,"iTotalRecords":2,"iTotalDisplayRecords":2,"aaData":[
{"QDIILanguage":"","assetNetValue":"","code":"110022","fund":{"idStr":2983},"gainPer":"","gainPerWan":"","shortName":"易方达消费行业股票","shareNetValue":"2.811","totalNetValue":"2.811","valuationDate":"2026-09-18","yearSevenDayYieldRatePercent":""},
{"QDIILanguage":"","assetNetValue":"","code":"110022","fund":{"idStr":2983},"gainPer":"","gainPerWan":"","shortName":"易方达消费行业股票","shareNetValue":"2.814","totalNetValue":"2.814","valuationDate":"2026-09-17","yearSevenDayYieldRatePercent":""}]}"#;

const MONEY_FUND_PAYLOAD: &str = r#"{"sEcho":1,"iTotalRecords":2,"iTotalDisplayRecords":2,"aaData":[
{"QDIILanguage":"","assetNetValue":"","code":"000198","fund":{"idStr":578},"gainPer":"0.2274","gainPerWan":"","shortName":"天弘余额宝货币","shareNetValue":"","totalNetValue":"","valuationDate":"2026-09-18","yearSevenDayYieldRatePercent":"0.8230%"},
{"QDIILanguage":"","assetNetValue":"","code":"000198","fund":{"idStr":578},"gainPer":"0.2233","gainPerWan":"","shortName":"天弘余额宝货币","shareNetValue":"","totalNetValue":"","valuationDate":"2026-09-17","yearSevenDayYieldRatePercent":"0.8200%"}]}"#;

const TERMINATED_FUND_PAYLOAD: &str = r#"{"sEcho":1,"iTotalRecords":2,"iTotalDisplayRecords":2,"aaData":[
{"QDIILanguage":"","assetNetValue":"","code":"002503","fund":{"idStr":3277},"gainPer":"","gainPerWan":"","shortName":"中银腾利混合C","shareNetValue":"1.144","totalNetValue":"1.415","valuationDate":"2023-09-18","yearSevenDayYieldRatePercent":""},
{"QDIILanguage":"","assetNetValue":"","code":"002503","fund":{"idStr":3277},"gainPer":"","gainPerWan":"","shortName":"中银腾利混合C","shareNetValue":"1.137","totalNetValue":"1.408","valuationDate":"2023-09-15","yearSevenDayYieldRatePercent":""}]}"#;

const MIXED_MONEY_FUND_PAYLOAD: &str = r#"{"sEcho":1,"iTotalRecords":2,"iTotalDisplayRecords":2,"aaData":[
{"QDIILanguage":"","assetNetValue":"","code":"000905","fund":{"idStr":620},"gainPer":"","gainPerWan":"","shortName":"鹏华安盈宝货币","shareNetValue":"","totalNetValue":"","valuationDate":"2026-09-18","yearSevenDayYieldRatePercent":""},
{"QDIILanguage":"","assetNetValue":"","code":"000905","fund":{"idStr":620},"gainPer":"0.3123","gainPerWan":"","shortName":"鹏华安盈宝货币A","shareNetValue":"","totalNetValue":"","valuationDate":"2026-09-18","yearSevenDayYieldRatePercent":"1.1710%"}]}"#;

const MIXED_NORMAL_FUND_PAYLOAD: &str = r#"{"sEcho":1,"iTotalRecords":2,"iTotalDisplayRecords":2,"aaData":[
{"QDIILanguage":"","assetNetValue":"","code":"020002","fund":{"idStr":762},"gainPer":"","gainPerWan":"","shortName":"国泰金龙债券","shareNetValue":"","totalNetValue":"","valuationDate":"2026-09-18","yearSevenDayYieldRatePercent":""},
{"QDIILanguage":"","assetNetValue":"","code":"020002","fund":{"idStr":762},"gainPer":"","gainPerWan":"","shortName":"国泰金龙债券A","shareNetValue":"1.1956","totalNetValue":"2.0128","valuationDate":"2026-09-18","yearSevenDayYieldRatePercent":""}]}"#;

const TRUSTED_EMPTY_PAYLOAD: &str =
    r#"{"sEcho":1,"iTotalRecords":0,"iTotalDisplayRecords":0,"aaData":[]}"#;

const SERVER_ERROR_HTML: &str = r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd"><html xmlns="http://www.w3.org/1999/xh"><head><title>系统异常</title></head><body><h2><em>系统异常</em></h2></body></html>"#;

fn assert_malformed(err: ledger_infra::error::AppError, label: &str) {
    assert!(
        err.is_code("sync.disclosure-source-malformed"),
        "{label} 应报 sync.disclosure-source-malformed 码化错误（fail-closed），实际 {err:?}"
    );
}

fn assert_parse_detail(detail: String, label: &str) {
    assert!(
        !detail.is_empty(),
        "{label} 应 fail-closed 并携带错误详情（解析器改回 Err(detail)）"
    );
}

#[test]
fn parse_pins_normal_fund_real_payload() {
    let page = parse_disclosure_page(NORMAL_FUND_PAYLOAD, "110022").unwrap();
    assert_eq!(page.records.len(), 2, "单日单行，无汇总行可滤");
    assert_eq!(page.total, 2, "声明行数与正文行数一致（裁剪快照保持自洽）");
    assert_eq!(
        page.records,
        vec![
            CsrcNavRecord {
                code: "110022".into(),
                name: "易方达消费行业股票".into(),
                valuation_date: "2026-09-18".into(),
                unit_nav: Some(2.811),
                accumulated_nav: Some(2.811),
                gain_per: None,
                seven_day_yield_percent: None,
            },
            CsrcNavRecord {
                code: "110022".into(),
                name: "易方达消费行业股票".into(),
                valuation_date: "2026-09-17".into(),
                unit_nav: Some(2.814),
                accumulated_nav: Some(2.814),
                gain_per: None,
                seven_day_yield_percent: None,
            },
        ],
        "记录按服务端原序（净值日期降序），单位/累计净值原样"
    );
    assert!(
        !page.records[0].is_money_fund_form(),
        "普通基金非货基自报形态"
    );
}

#[test]
fn parse_pins_money_fund_self_report_form() {
    let page = parse_disclosure_page(MONEY_FUND_PAYLOAD, "000198").unwrap();
    assert_eq!(page.records.len(), 2);
    let first = &page.records[0];
    assert_eq!(first.name, "天弘余额宝货币");
    assert_eq!(first.valuation_date, "2026-09-18");
    assert_eq!(first.unit_nav, None, "货基自报：单位净值为空");
    assert_eq!(first.accumulated_nav, None, "货基自报：累计净值为空");
    assert_eq!(first.gain_per, Some(0.2274), "万份收益有值");
    assert_eq!(
        first.seven_day_yield_percent,
        Some(0.823),
        "七日年化有值（% 后缀剥离为数值）"
    );
    assert!(first.is_money_fund_form(), "三信号齐备即货基自报形态");
}

#[test]
fn parse_pins_terminated_fund_last_disclosure() {
    let page = parse_disclosure_page(TERMINATED_FUND_PAYLOAD, "002503").unwrap();
    let last = &page.records[0];
    assert_eq!(last.name, "中银腾利混合C");
    assert_eq!(last.valuation_date, "2023-09-18");
    assert_eq!(last.unit_nav, Some(1.144));
    assert_eq!(last.accumulated_nav, Some(1.415));
}

#[test]
fn parse_filters_summary_row_and_keeps_valued_share_row() {
    for (payload, code, expected_name) in [
        (MIXED_MONEY_FUND_PAYLOAD, "000905", "鹏华安盈宝货币A"),
        (MIXED_NORMAL_FUND_PAYLOAD, "020002", "国泰金龙债券A"),
    ] {
        let page = parse_disclosure_page(payload, code).unwrap();
        assert_eq!(
            page.records.len(),
            1,
            "同日两条记录（汇总行 + 份额行）应只留份额行"
        );
        let record = &page.records[0];
        assert_eq!(record.name, expected_name, "取的是份额行（名称带份额后缀）");
        assert_eq!(record.valuation_date, "2026-09-18");
    }
    let money = parse_disclosure_page(MIXED_MONEY_FUND_PAYLOAD, "000905").unwrap();
    assert!(money.records[0].is_money_fund_form());
    let normal = parse_disclosure_page(MIXED_NORMAL_FUND_PAYLOAD, "020002").unwrap();
    assert_eq!(normal.records[0].unit_nav, Some(1.1956));
    assert_eq!(normal.records[0].accumulated_nav, Some(2.0128));
    assert!(!normal.records[0].is_money_fund_form());
}

#[test]
fn parse_trusted_empty_is_ok_empty() {
    let page = parse_disclosure_page(TRUSTED_EMPTY_PAYLOAD, "999999").unwrap();
    assert_eq!(page.records.len(), 0);
    assert_eq!(page.total, 0);
}

#[test]
fn parse_rejects_untrusted_shapes_fail_closed() {
    let untrusted = [
        ("空响应", ""),
        ("非 JSON 拦截页", "<html>risk control page</html>"),
        ("500 系统异常页", SERVER_ERROR_HTML),
        ("对象缺 aaData", r#"{"sEcho":1,"iTotalRecords":0}"#),
        (
            "JSON 截断",
            r#"{"iTotalRecords":1,"aaData":[{"code":"11002"#,
        ),
        (
            "服务端自报失败",
            r#"{"success":false,"message":"系统异常"}"#,
        ),
        ("aaData 非数组", r#"{"iTotalRecords":1,"aaData":{}}"#),
    ];
    for (label, body) in untrusted {
        assert_parse_detail(parse_disclosure_page(body, "110022").unwrap_err(), label);
    }
}

#[test]
fn parse_tolerates_numeric_and_null_wire_forms() {
    let body = r#"{"iTotalRecords":2,"aaData":[
        {"code":"110022","shortName":"易方达消费行业股票","shareNetValue":2.811,"totalNetValue":null,"valuationDate":"2026-09-18","gainPer":"","yearSevenDayYieldRatePercent":null},
        {"code":110022,"shortName":"数字代码","shareNetValue":"1.5","totalNetValue":"","valuationDate":"2026-09-17","gainPer":0,"yearSevenDayYieldRatePercent":"0%"}
    ]}"#;
    let page = parse_disclosure_page(body, "110022").unwrap();
    assert_eq!(
        page.records.len(),
        2,
        "两行都可解析（同码防守对数字代码宽容）"
    );
    assert_eq!(page.records[0].unit_nav, Some(2.811));
    assert_eq!(page.records[0].accumulated_nav, None);
    assert_eq!(page.records[1].gain_per, Some(0.0));
    assert_eq!(page.records[1].seven_day_yield_percent, Some(0.0));
}

#[test]
fn parse_drops_rows_failing_code_or_date_discipline() {
    let body = r#"{"iTotalRecords":3,"aaData":[
        {"code":"161725","shortName":"其他基金","shareNetValue":"1.0","totalNetValue":"","valuationDate":"2026-09-18","gainPer":"","yearSevenDayYieldRatePercent":""},
        {"code":"110022","shortName":"易方达消费行业股票","shareNetValue":"2.811","totalNetValue":"2.811","valuationDate":"","gainPer":"","yearSevenDayYieldRatePercent":""},
        {"code":"110022","shortName":"易方达消费行业股票","shareNetValue":"2.814","totalNetValue":"2.814","valuationDate":"2026-09-17","gainPer":"","yearSevenDayYieldRatePercent":""}
    ]}"#;
    let page = parse_disclosure_page(body, "110022").unwrap();
    assert_eq!(
        page.records.len(),
        1,
        "他码行与缺日期行被防御性过滤，只留同码带日期行"
    );
    assert_eq!(page.records[0].valuation_date, "2026-09-17");
}

#[test]
fn parse_all_rows_filtered_by_discipline_fails_closed() {
    let code_drift = r#"{"iTotalRecords":1,"aaData":[
        {"code":"999999","shortName":"易方达消费行业股票","shareNetValue":"2.811","totalNetValue":"","valuationDate":"2026-09-18","gainPer":"","yearSevenDayYieldRatePercent":""}
    ]}"#;
    assert_parse_detail(
        parse_disclosure_page(code_drift, "110022").unwrap_err(),
        "同码防守整页滤空",
    );
    let date_drift = r#"{"iTotalRecords":1,"aaData":[
        {"code":"110022","shortName":"易方达消费行业股票","shareNetValue":"2.811","totalNetValue":"","valuationDate":"","gainPer":"","yearSevenDayYieldRatePercent":""}
    ]}"#;
    assert_parse_detail(
        parse_disclosure_page(date_drift, "110022").unwrap_err(),
        "净值日期整体缺失",
    );
}

fn decoded_ao_data(head: &str) -> String {
    let request_line = head.lines().next().expect("请求行应存在");
    let query = request_line
        .split_once(' ')
        .expect("请求行应有目标")
        .1
        .split_once('?')
        .expect("请求应携带查询串")
        .1
        .split(" HTTP/")
        .next()
        .unwrap();
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(name, _)| name == "aoData")
        .expect("查询串应携带 aoData 参数")
        .1
        .into_owned()
}

fn fast_pacer() -> crate::http::Pacer {
    crate::http::Pacer::new(Duration::ZERO)
}

#[test]
fn fetch_pins_request_shape_with_full_datatables_params() {
    let (url, heads) = spawn_capture_server(|_| (200, NORMAL_FUND_PAYLOAD.into()));
    let client = reqwest::Client::new();
    let mut pacer = fast_pacer();
    let records = tauri::async_runtime::block_on(fetch_fund_nav_series_from(
        &client,
        &mut pacer,
        "110022",
        "2026-09-17",
        "2026-09-18",
        &[url.as_str()],
    ))
    .unwrap();
    assert_eq!(records.len(), 2, "响应照常解析为净值记录");

    let head = &heads.lock().unwrap()[0];
    let request_line = head.lines().next().unwrap();
    assert!(
        request_line.starts_with("GET /fund/disclose/getPublicFundJZInfoMore.do?"),
        "请求行应为官方披露区间查询接口: {request_line}"
    );
    let ao = decoded_ao_data(head);
    for needle in [
        r#""name":"sEcho""#,
        r#""name":"iColumns","value":5"#,
        r#""name":"sColumns""#,
        r#""name":"iDisplayStart","value":0"#,
        r#""name":"iDisplayLength","value":500"#,
        r#""name":"mDataProp_0""#,
        r#""name":"mDataProp_4""#,
        r#""name":"sSearch""#,
        r#""name":"bRegex","value":false"#,
        r#""name":"sSearch_0""#,
        r#""name":"bRegex_0""#,
        r#""name":"bSortable_0""#,
        r#""name":"bSortable_4""#,
    ] {
        assert!(ao.contains(needle), "aoData 应含 {needle}: {ao}");
    }
    for needle in [
        r#""name":"fundType","value":"all""#,
        r#""name":"fundCompanyShortName","value":"""#,
        r#""name":"fundCode","value":"110022""#,
        r#""name":"fundName","value":"""#,
        r#""name":"startDate","value":"2026-09-17""#,
        r#""name":"endDate","value":"2026-09-18""#,
    ] {
        assert!(ao.contains(needle), "aoData 应含 {needle}: {ao}");
    }
}

#[test]
fn fetch_trusted_empty_from_server_is_ok_empty() {
    let (url, _) = spawn_capture_server(|_| (200, TRUSTED_EMPTY_PAYLOAD.into()));
    let client = reqwest::Client::new();
    let mut pacer = fast_pacer();
    let records = tauri::async_runtime::block_on(fetch_fund_nav_series_from(
        &client,
        &mut pacer,
        "999999",
        "2026-09-17",
        "2026-09-18",
        &[url.as_str()],
    ))
    .unwrap();
    assert!(records.is_empty(), "可信空是合法结果");
}

#[test]
fn fetch_abnormal_responses_fail_closed() {
    for (label, status, body) in [
        ("500 系统异常页", 500u16, SERVER_ERROR_HTML.to_string()),
        ("200 空响应", 200, String::new()),
        (
            "200 非 JSON 拦截页",
            200,
            "<html>blocked</html>".to_string(),
        ),
        (
            "200 报文缺 aaData",
            200,
            r#"{"sEcho":1,"iTotalRecords":0}"#.to_string(),
        ),
    ] {
        let (url, _) = spawn_capture_server(move |_| (status, body.clone().into()));
        let client = reqwest::Client::new();
        let mut pacer = fast_pacer();
        let result = tauri::async_runtime::block_on(fetch_fund_nav_series_from(
            &client,
            &mut pacer,
            "110022",
            "2026-09-17",
            "2026-09-18",
            &[url.as_str()],
        ));
        assert_malformed(result.unwrap_err(), label);
    }
}

fn synthetic_row(code: &str, date: &str, unit_nav: &str) -> String {
    format!(
        r#"{{"code":"{code}","shortName":"合成基金","shareNetValue":"{unit_nav}","totalNetValue":"","valuationDate":"{date}","gainPer":"","yearSevenDayYieldRatePercent":""}}"#
    )
}

#[test]
fn fetch_follows_declared_total_across_pages() {
    let page1 = format!(
        r#"{{"sEcho":1,"iTotalRecords":600,"iTotalDisplayRecords":600,"aaData":[{}]}}"#,
        (0..500)
            .map(|i| synthetic_row("110022", &format!("2026-09-{:02}", 18 - i / 40), "1.0"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let page2 = format!(
        r#"{{"sEcho":2,"iTotalRecords":600,"iTotalDisplayRecords":600,"aaData":[{}]}}"#,
        (0..100)
            .map(|i| synthetic_row("110022", &format!("2024-09-{:02}", 28 - i / 8), "1.0"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let (url, heads) = spawn_capture_server(move |n| {
        if n == 1 {
            (200, page1.clone().into())
        } else {
            (200, page2.clone().into())
        }
    });
    let client = reqwest::Client::new();
    let mut pacer = fast_pacer();
    let records = tauri::async_runtime::block_on(fetch_fund_nav_series_from(
        &client,
        &mut pacer,
        "110022",
        "2024-09-19",
        "2026-09-18",
        &[url.as_str()],
    ))
    .unwrap();
    assert_eq!(records.len(), 600, "两页行数拼接齐全");
    let heads = heads.lock().unwrap();
    assert_eq!(heads.len(), 2, "按声明总数恰发两次请求");
    let second_ao = decoded_ao_data(&heads[1]);
    assert!(
        second_ao.contains(r#""name":"iDisplayStart","value":500"#),
        "第二页请求从 500 行起: {second_ao}"
    );
}

#[test]
fn fetch_window_deeper_than_page_cap_fails_closed() {
    let full_page = format!(
        r#"{{"sEcho":1,"iTotalRecords":100000,"iTotalDisplayRecords":100000,"aaData":[{}]}}"#,
        (0..500)
            .map(|i| synthetic_row("110022", &format!("2026-09-{:02}", 18 - i / 40), "1.0"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let (url, heads) = spawn_capture_server(move |_| (200, full_page.clone().into()));
    let client = reqwest::Client::new();
    let mut pacer = fast_pacer();
    let result = tauri::async_runtime::block_on(fetch_fund_nav_series_from(
        &client,
        &mut pacer,
        "110022",
        "2000-01-01",
        "2026-09-18",
        &[url.as_str()],
    ));
    assert_malformed(result.unwrap_err(), "窗口过深");
    assert_eq!(
        heads.lock().unwrap().len(),
        1,
        "窗口过深在首页即判定，不发无界请求"
    );
}

#[test]
fn fetch_incomplete_paging_fails_closed() {
    let page1 = format!(
        r#"{{"sEcho":1,"iTotalRecords":600,"iTotalDisplayRecords":600,"aaData":[{}]}}"#,
        (0..500)
            .map(|i| synthetic_row("110022", &format!("2026-09-{:02}", 18 - i / 40), "1.0"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let (url, _) = spawn_capture_server(move |n| {
        if n == 1 {
            (200, page1.clone().into())
        } else {
            (200, TRUSTED_EMPTY_PAYLOAD.into())
        }
    });
    let client = reqwest::Client::new();
    let mut pacer = fast_pacer();
    let result = tauri::async_runtime::block_on(fetch_fund_nav_series_from(
        &client,
        &mut pacer,
        "110022",
        "2024-09-19",
        "2026-09-18",
        &[url.as_str()],
    ));
    assert_malformed(result.unwrap_err(), "翻页不完整");
}

#[test]
fn confirm_money_fund_form_pins_money_fund_self_report() {
    let (url, heads) = spawn_capture_server(|_| (200, MONEY_FUND_PAYLOAD.into()));
    let client = reqwest::Client::new();
    let mut pacer = fast_pacer();
    let confirmed = tauri::async_runtime::block_on(confirm_money_fund_form_from(
        &client,
        &mut pacer,
        "000198",
        &[url.as_str()],
    ))
    .unwrap();
    assert!(confirmed, "货基自报形态确认");

    let (head, served) = {
        let guard = heads.lock().unwrap();
        (guard[0].clone(), guard.len())
    };
    let ao = decoded_ao_data(&head);
    assert!(
        ao.contains(r#""name":"iDisplayStart","value":0"#),
        "确认请求应为最新一页: {ao}"
    );
    assert_eq!(served, 1, "确认不翻页、单请求");
}

#[test]
fn confirm_normal_fund_and_trusted_empty_are_missing_signal() {
    let (url, _) = spawn_capture_server(|_| (200, NORMAL_FUND_PAYLOAD.into()));
    let client = reqwest::Client::new();
    let mut pacer = fast_pacer();
    let confirmed = tauri::async_runtime::block_on(confirm_money_fund_form_from(
        &client,
        &mut pacer,
        "110022",
        &[url.as_str()],
    ))
    .unwrap();
    assert!(!confirmed, "普通净值形态缺信号");

    let (url, _) = spawn_capture_server(|_| (200, TERMINATED_FUND_PAYLOAD.into()));
    let mut pacer = fast_pacer();
    let confirmed = tauri::async_runtime::block_on(confirm_money_fund_form_from(
        &client,
        &mut pacer,
        "002503",
        &[url.as_str()],
    ))
    .unwrap();
    assert!(!confirmed, "已终止基金普通形态缺信号");

    let (url, _) = spawn_capture_server(|_| (200, TRUSTED_EMPTY_PAYLOAD.into()));
    let mut pacer = fast_pacer();
    let confirmed = tauri::async_runtime::block_on(confirm_money_fund_form_from(
        &client,
        &mut pacer,
        "999999",
        &[url.as_str()],
    ))
    .unwrap();
    assert!(!confirmed, "可信空是缺信号而非错误");
}

#[test]
fn confirm_summary_row_mixed_page_confirms_by_share_row() {
    let (url, _) = spawn_capture_server(|_| (200, MIXED_MONEY_FUND_PAYLOAD.into()));
    let client = reqwest::Client::new();
    let mut pacer = fast_pacer();
    let confirmed = tauri::async_runtime::block_on(confirm_money_fund_form_from(
        &client,
        &mut pacer,
        "000905",
        &[url.as_str()],
    ))
    .unwrap();
    assert!(confirmed, "混排页按份额行形态确认");
}

#[test]
fn confirm_untrusted_response_fails_closed() {
    for (label, status, body) in [
        ("500 系统异常页", 500u16, SERVER_ERROR_HTML.to_string()),
        (
            "200 非 JSON 拦截页",
            200,
            "<html>blocked</html>".to_string(),
        ),
        (
            "对象缺 aaData",
            200,
            r#"{"sEcho":1,"iTotalRecords":0}"#.to_string(),
        ),
    ] {
        let (url, _) = spawn_capture_server(move |_| (status, body.clone().into()));
        let client = reqwest::Client::new();
        let mut pacer = fast_pacer();
        let result = tauri::async_runtime::block_on(confirm_money_fund_form_from(
            &client,
            &mut pacer,
            "110022",
            &[url.as_str()],
        ));
        assert_malformed(result.unwrap_err(), label);
    }
}
