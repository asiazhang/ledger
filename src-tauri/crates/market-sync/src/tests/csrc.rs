//! 证监会基金电子披露取数单元（issue #1562 / ADR-0130）：区间查询报文解析
//! （fixture 为 2026-09-20 实测真实报文形状，不依赖真实网络）与请求形态钉值
//! （本地 HTTP 服务）。
//!
//! 覆盖四类真实记录形态：普通基金、货币基金（自报形态）、已终止基金、汇总行
//! 与份额行混排；以及参数门槛与异常响应的 fail-closed 语义——缺参数的 500
//! 「系统异常」页、空响应、非 JSON 拦截页一律报错上抛，绝不落「无数据」结论。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::csrc::{
    CsrcNavRecord, confirm_money_fund_form_from, fetch_fund_nav_series_from, parse_disclosure_page,
};

// ---------------------------------------------------------------------------
// 真实报文 fixture（2026-09-20 实测 eid.csrc.gov.cn，字段截取保持真实键序与值）
// ---------------------------------------------------------------------------

/// 普通基金 110022（易方达消费行业股票）近两日：单日单行、单位/累计净值齐备、
/// 收益字段为空串；行内大量未消费字段按真实形态保留代表性几个。
const NORMAL_FUND_PAYLOAD: &str = r#"{"sEcho":1,"iTotalRecords":2,"iTotalDisplayRecords":2,"aaData":[
{"QDIILanguage":"","assetNetValue":"","code":"110022","fund":{"idStr":2983},"gainPer":"","gainPerWan":"","shortName":"易方达消费行业股票","shareNetValue":"2.811","totalNetValue":"2.811","valuationDate":"2026-09-18","yearSevenDayYieldRatePercent":""},
{"QDIILanguage":"","assetNetValue":"","code":"110022","fund":{"idStr":2983},"gainPer":"","gainPerWan":"","shortName":"易方达消费行业股票","shareNetValue":"2.814","totalNetValue":"2.814","valuationDate":"2026-09-17","yearSevenDayYieldRatePercent":""}]}"#;

/// 货币基金 000198（天弘余额宝货币）自报形态：单位净值与累计净值为空串、
/// 万份收益（gainPer）与七日年化（带 % 后缀的字符串）有值。
const MONEY_FUND_PAYLOAD: &str = r#"{"sEcho":1,"iTotalRecords":2,"iTotalDisplayRecords":2,"aaData":[
{"QDIILanguage":"","assetNetValue":"","code":"000198","fund":{"idStr":578},"gainPer":"0.2274","gainPerWan":"","shortName":"天弘余额宝货币","shareNetValue":"","totalNetValue":"","valuationDate":"2026-09-18","yearSevenDayYieldRatePercent":"0.8230%"},
{"QDIILanguage":"","assetNetValue":"","code":"000198","fund":{"idStr":578},"gainPer":"0.2233","gainPerWan":"","shortName":"天弘余额宝货币","shareNetValue":"","totalNetValue":"","valuationDate":"2026-09-17","yearSevenDayYieldRatePercent":"0.8200%"}]}"#;

/// 已终止（清盘）基金 002503（中银腾利混合C）2023-09 的最后两期披露：末点
/// 1.144 / 2023-09-18，与东财档案通道及调研样本逐值一致（13.5 节）。
const TERMINATED_FUND_PAYLOAD: &str = r#"{"sEcho":1,"iTotalRecords":2,"iTotalDisplayRecords":2,"aaData":[
{"QDIILanguage":"","assetNetValue":"","code":"002503","fund":{"idStr":3277},"gainPer":"","gainPerWan":"","shortName":"中银腾利混合C","shareNetValue":"1.144","totalNetValue":"1.415","valuationDate":"2023-09-18","yearSevenDayYieldRatePercent":""},
{"QDIILanguage":"","assetNetValue":"","code":"002503","fund":{"idStr":3277},"gainPer":"","gainPerWan":"","shortName":"中银腾利混合C","shareNetValue":"1.137","totalNetValue":"1.408","valuationDate":"2023-09-15","yearSevenDayYieldRatePercent":""}]}"#;

/// 汇总行与份额行混排（货币基金 000905）：同一净值日期两条记录——汇总行
/// （名称不带份额后缀、字段全空）在前、份额行（名称带 A 后缀、字段有值）在后；
/// 只看第一条会取到空净值（13.5 节记录形态陷阱）。
const MIXED_MONEY_FUND_PAYLOAD: &str = r#"{"sEcho":1,"iTotalRecords":2,"iTotalDisplayRecords":2,"aaData":[
{"QDIILanguage":"","assetNetValue":"","code":"000905","fund":{"idStr":620},"gainPer":"","gainPerWan":"","shortName":"鹏华安盈宝货币","shareNetValue":"","totalNetValue":"","valuationDate":"2026-09-18","yearSevenDayYieldRatePercent":""},
{"QDIILanguage":"","assetNetValue":"","code":"000905","fund":{"idStr":620},"gainPer":"0.3123","gainPerWan":"","shortName":"鹏华安盈宝货币A","shareNetValue":"","totalNetValue":"","valuationDate":"2026-09-18","yearSevenDayYieldRatePercent":"1.1710%"}]}"#;

/// 汇总行与份额行混排（普通基金 020002 国泰金龙债券）：同一净值日期汇总行全空、
/// 份额行带单位净值与累计净值——混排不是货基专属形态，普通基金同样出现。
const MIXED_NORMAL_FUND_PAYLOAD: &str = r#"{"sEcho":1,"iTotalRecords":2,"iTotalDisplayRecords":2,"aaData":[
{"QDIILanguage":"","assetNetValue":"","code":"020002","fund":{"idStr":762},"gainPer":"","gainPerWan":"","shortName":"国泰金龙债券","shareNetValue":"","totalNetValue":"","valuationDate":"2026-09-18","yearSevenDayYieldRatePercent":""},
{"QDIILanguage":"","assetNetValue":"","code":"020002","fund":{"idStr":762},"gainPer":"","gainPerWan":"","shortName":"国泰金龙债券A","shareNetValue":"1.1956","totalNetValue":"2.0128","valuationDate":"2026-09-18","yearSevenDayYieldRatePercent":""}]}"#;

/// 查无此码 / 窗口内无披露的真实形态：报文结构完好、aaData 为空数组——这是
/// 唯一允许的「无数据」结论来源（可信空）。
const TRUSTED_EMPTY_PAYLOAD: &str =
    r#"{"sEcho":1,"iTotalRecords":0,"iTotalDisplayRecords":0,"aaData":[]}"#;

/// 缺 DataTables 参数时服务端的真实形态：HTTP 500 + 「系统异常」HTML 页
///（不是空数据——解析层必须把它与「查无此码」区分开）。
const SERVER_ERROR_HTML: &str = r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd"><html xmlns="http://www.w3.org/1999/xh"><head><title>系统异常</title></head><body><h2><em>系统异常</em></h2></body></html>"#;

fn assert_malformed(err: ledger_infra::error::AppError, label: &str) {
    assert!(
        err.is_code("sync.disclosure-source-malformed"),
        "{label} 应报 sync.disclosure-source-malformed 码化错误（fail-closed），实际 {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 解析层：真实报文 fixture 钉值
// ---------------------------------------------------------------------------

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
    // 货基自报形态（ADR-0126 决策 3 换源后的判定信号）：单位净值为空、万份收益
    // 与七日年化有值。万份收益与七日年化数值按事实保留（% 后缀剥离），不进价格。
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
    // 已终止基金仍可取到最后一期净值（ADR-0130 存在性兜底的事实依据）。
    let page = parse_disclosure_page(TERMINATED_FUND_PAYLOAD, "002503").unwrap();
    let last = &page.records[0];
    assert_eq!(last.name, "中银腾利混合C");
    assert_eq!(last.valuation_date, "2023-09-18");
    assert_eq!(last.unit_nav, Some(1.144));
    assert_eq!(last.accumulated_nav, Some(1.415));
}

#[test]
fn parse_filters_summary_row_and_keeps_valued_share_row() {
    // 汇总行（名称不带份额后缀、字段全空）必须滤除，只有带值的份额行参与取值；
    // 两个 fixture 分别覆盖货基与普通基金的混排形态。
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
    // 货基混排的份额行仍是货基自报形态。
    let money = parse_disclosure_page(MIXED_MONEY_FUND_PAYLOAD, "000905").unwrap();
    assert!(money.records[0].is_money_fund_form());
    // 普通基金混排的份额行带齐两种净值。
    let normal = parse_disclosure_page(MIXED_NORMAL_FUND_PAYLOAD, "020002").unwrap();
    assert_eq!(normal.records[0].unit_nav, Some(1.1956));
    assert_eq!(normal.records[0].accumulated_nav, Some(2.0128));
    assert!(!normal.records[0].is_money_fund_form());
}

#[test]
fn parse_trusted_empty_is_ok_empty() {
    // 报文结构完好、aaData 为空数组：可信空结果（查无此码 / 窗口内无披露），
    // 与「数据坏了」严格区分——这是唯一允许的「无数据」结论来源。
    let page = parse_disclosure_page(TRUSTED_EMPTY_PAYLOAD, "999999").unwrap();
    assert_eq!(page.records.len(), 0);
    assert_eq!(page.total, 0);
}

#[test]
fn parse_rejects_untrusted_shapes_fail_closed() {
    // 异常响应一律 Err（fail-closed），绝不静默产出空序列——空序列会让
    // 「查无此码」与「数据源坏了」不可分辨，后者被误判为前者是本票明令禁止的。
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
        assert_malformed(parse_disclosure_page(body, "110022").unwrap_err(), label);
    }
}

#[test]
fn parse_tolerates_numeric_and_null_wire_forms() {
    // 数值字段可能以数字而非字符串出现（宽容解析：数字 → 数值）；null 与空串
    // 归「无值」；行不因字段形态漂移而整页失败。
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
    // 万份收益 0 是「有值」（收益披露口径下的零收益日），不等于「无值」。
    assert_eq!(page.records[1].gain_per, Some(0.0));
    assert_eq!(page.records[1].seven_day_yield_percent, Some(0.0));
}

#[test]
fn parse_drops_rows_failing_code_or_date_discipline() {
    // 同码防守：区间查询按单代码圈定，混入的其他代码行不属于本次查询；
    // 缺净值日期的行无处落位；两行都不产出记录、也不影响其余行。
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
    // 有行但全部未通过解析纪律（字段漂移使同码防守整页滤空 / 日期整体缺失）：
    // 不可信 → Err，绝不静默产出空序列——那会把「数据坏了」误判成「查无此码」。
    let code_drift = r#"{"iTotalRecords":1,"aaData":[
        {"code":"999999","shortName":"易方达消费行业股票","shareNetValue":"2.811","totalNetValue":"","valuationDate":"2026-09-18","gainPer":"","yearSevenDayYieldRatePercent":""}
    ]}"#;
    assert_malformed(
        parse_disclosure_page(code_drift, "110022").unwrap_err(),
        "同码防守整页滤空",
    );
    let date_drift = r#"{"iTotalRecords":1,"aaData":[
        {"code":"110022","shortName":"易方达消费行业股票","shareNetValue":"2.811","totalNetValue":"","valuationDate":"","gainPer":"","yearSevenDayYieldRatePercent":""}
    ]}"#;
    assert_malformed(
        parse_disclosure_page(date_drift, "110022").unwrap_err(),
        "净值日期整体缺失",
    );
}

// ---------------------------------------------------------------------------
// 取数层：本地 HTTP 服务钉住请求形态（不依赖真实网络）
// ---------------------------------------------------------------------------

/// 起一个按调用次数回调响应 (status, body) 并按序收集请求头的本地 HTTP 服务。
fn spawn_capture_server(
    responder: impl Fn(usize) -> (u16, String) + Send + 'static,
) -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let heads = Arc::new(Mutex::new(Vec::new()));
    let heads_clone = heads.clone();
    std::thread::spawn(move || {
        let mut seq = 0usize;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let _ = stream.read(&mut buf);
            heads_clone
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf).to_string());
            seq += 1;
            let (status, body) = responder(seq);
            let reason = if status == 200 { "OK" } else { "Error" };
            let resp = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    (url, heads)
}

/// 从原始请求头里取出 aoData 参数值并百分号解码（请求形态断言用）。
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
    let raw = query
        .split('&')
        .find(|pair| pair.starts_with("aoData="))
        .expect("查询串应携带 aoData 参数");
    let encoded = raw.strip_prefix("aoData=").unwrap();
    let bytes = encoded.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                i += 3;
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap()
}

fn fast_pacer() -> crate::http::Pacer {
    crate::http::Pacer::new(Duration::ZERO)
}

#[test]
fn fetch_pins_request_shape_with_full_datatables_params() {
    // 参数门槛（13.5 节）：官方站缺参数返回 500 系统异常而非空数据——请求必须
    // 完整携带 DataTables 风格参数与业务参数。本用例把请求形态钉住。
    let (url, heads) = spawn_capture_server(|_| (200, NORMAL_FUND_PAYLOAD.to_string()));
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
    // DataTables 风格参数全量在场（缺任一即 500 系统异常的参数门槛）。
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
    // 业务参数：单代码 + 日期闭区间（站点 JS 的同款业务字段）。
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
    // 查无此码的服务端形态（结构完好 + aaData 空）才允许得出「无数据」结论。
    let (url, _) = spawn_capture_server(|_| (200, TRUSTED_EMPTY_PAYLOAD.to_string()));
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
    // 异常响应（500 系统异常页 / 200 空响应 / 非 JSON 拦截页 / 报文缺 aaData）
    // 一律报错上抛，绝不落「无数据」结论——把不可信结果当空窗口会直接误判
    // 「查无此码」。
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
        let (url, _) = spawn_capture_server(move |_| (status, body.clone()));
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

/// 生成一行披露记录的 JSON（分页用例的合成行，字段形态与真实报文一致）。
fn synthetic_row(code: &str, date: &str, unit_nav: &str) -> String {
    format!(
        r#"{{"code":"{code}","shortName":"合成基金","shareNetValue":"{unit_nav}","totalNetValue":"","valuationDate":"{date}","gainPer":"","yearSevenDayYieldRatePercent":""}}"#
    )
}

#[test]
fn fetch_follows_declared_total_across_pages() {
    // 服务端声明 600 行：第一页满页（500 行，iDisplayStart=0），第二页补尾
    //（100 行，iDisplayStart=500）——翻页由页 1 声明的总数驱动。
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
            (200, page1.clone())
        } else {
            (200, page2.clone())
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
    // 声明总数超出页数上限（窗口过深）：fail-closed 报错，不发无界请求；
    // 绝不静默截断成半截窗口冒充完整数据。
    let full_page = format!(
        r#"{{"sEcho":1,"iTotalRecords":100000,"iTotalDisplayRecords":100000,"aaData":[{}]}}"#,
        (0..500)
            .map(|i| synthetic_row("110022", &format!("2026-09-{:02}", 18 - i / 40), "1.0"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let (url, heads) = spawn_capture_server(move |_| (200, full_page.clone()));
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
    // 页 1 声明 600 行、满页返回；翻页页却被拦截（结构完好但空）——行数取不全
    // 即窗口不完整，fail-closed 报错而不是把前 500 行冒充完整数据。
    let page1 = format!(
        r#"{{"sEcho":1,"iTotalRecords":600,"iTotalDisplayRecords":600,"aaData":[{}]}}"#,
        (0..500)
            .map(|i| synthetic_row("110022", &format!("2026-09-{:02}", 18 - i / 40), "1.0"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let (url, _) = spawn_capture_server(move |n| {
        if n == 1 {
            (200, page1.clone())
        } else {
            (200, TRUSTED_EMPTY_PAYLOAD.to_string())
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

// ---------------------------------------------------------------------------
// 货基判定确认（issue #1563 / ADR-0126 决策 3 换源）：最新一页披露记录的
// 自报形态三态——确认 / 缺信号 / 源不可信。
// ---------------------------------------------------------------------------

#[test]
fn confirm_money_fund_form_pins_money_fund_self_report() {
    // 货基自报形态（真实报文 fixture）：最新记录「单位净值为空、万份收益与
    // 七日年化有值」即确认。
    let (url, heads) = spawn_capture_server(|_| (200, MONEY_FUND_PAYLOAD.to_string()));
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

    // 确认只取最新一页（display_start = 0），不发翻页请求——一次确认一次请求。
    // （锁守卫一次取齐：std Mutex 不可重入，持守卫再 lock 同线程即死锁。）
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
    // 普通净值形态记录：不是货基自报形态 → Ok(false)——缺信号不是「不是恒定
    // 标的」的反证，调用方不得据此清空既有标记。
    let (url, _) = spawn_capture_server(|_| (200, NORMAL_FUND_PAYLOAD.to_string()));
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

    // 已终止基金的老记录（普通净值形态）同样缺信号；可信空报文（查无此码 /
    // 窗口内无披露）同样缺信号、绝不报错。
    let (url, _) = spawn_capture_server(|_| (200, TERMINATED_FUND_PAYLOAD.to_string()));
    let mut pacer = fast_pacer();
    let confirmed = tauri::async_runtime::block_on(confirm_money_fund_form_from(
        &client,
        &mut pacer,
        "002503",
        &[url.as_str()],
    ))
    .unwrap();
    assert!(!confirmed, "已终止基金普通形态缺信号");

    let (url, _) = spawn_capture_server(|_| (200, TRUSTED_EMPTY_PAYLOAD.to_string()));
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
    // 混排形态（汇总行 + 份额行）：解析层滤掉空值汇总行后，最新份额行即货基
    // 自报形态——只看第一条原始行会取到全空的汇总行（13.5 节记录形态陷阱）。
    let (url, _) = spawn_capture_server(|_| (200, MIXED_MONEY_FUND_PAYLOAD.to_string()));
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
    // 披露源不可信（500 系统异常页 / 非 JSON 拦截页 / 缺 aaData）：Err 上抛，
    // 调用方本轮整只不落——不把不可信当「缺信号」。
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
        let (url, _) = spawn_capture_server(move |_| (status, body.clone()));
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
