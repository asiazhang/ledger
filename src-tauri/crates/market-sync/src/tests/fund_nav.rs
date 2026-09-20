//! 历史净值通道（issue #303 / ADR-0038 决策 6）：lsjz 报文解析（fixture 为真实
//! 接口形状）、净值同步水位窗口语义、Referer 头传播。全部离线驱动，不依赖真实
//! 网络；基金分区编排（水位增量回填的端到端语义）见 `instrument_info_sync.rs`。

use super::spawn_header_capture_server;
use std::time::Duration;

use chrono::NaiveDate;

use crate::fund_nav::{
    NavPoint, NavQuery, NavResponse, fetch_nav_page_from, nav_window, parse_lsjz,
};
use crate::http::{Pacer, request_json_from_hosts};

/// 真实 lsjz 响应形状（fundCode=110022，实测 2026-08）：Data.LSJZList 按净值
/// 日期降序，DWJZ 为数字字符串，TotalCount 在顶层。
const REAL_PAYLOAD: &str = r#"{"Data":{"LSJZList":[{"FSRQ":"2026-01-30","DWJZ":"3.3480","LJJZ":"3.3480","SDATE":null,"ACTUALSYI":"","NAVTYPE":"1","JZZZL":"-2.11","SGZT":"开放申购","SHZT":"开放赎回","FHFCZ":"","FHFCZ10":null,"FHFCBZ":"","DTYPE":null,"FHSP":""},{"FSRQ":"2026-01-29","DWJZ":"3.4200","LJJZ":"3.4200","SDATE":null,"ACTUALSYI":"","NAVTYPE":"1","JZZZL":"3.86","SGZT":"开放申购","SHZT":"开放赎回","FHFCZ":"","FHFCZ10":null,"FHFCBZ":"","DTYPE":null,"FHSP":""}],"FundType":"001","SYType":null,"isNewType":false,"Feature":null},"ErrCode":0,"ErrMsg":null,"TotalCount":506,"Expansion":null,"PageSize":5,"PageIndex":1}"#;

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[test]
fn lsjz_response_deserializes_real_payload() {
    let resp: NavResponse = serde_json::from_str(REAL_PAYLOAD).unwrap();
    assert_eq!(resp.total_count, 506);
    let parsed = parse_lsjz(&resp);
    assert!(!parsed.blocked, "正常对象报文不是空响应");
    assert_eq!(parsed.points.len(), 2);
    assert_eq!(parsed.points[0].date, "2026-01-30");
    assert_eq!(parsed.points[0].nav, 3.3480);
    assert_eq!(parsed.points[1].date, "2026-01-29");
    assert_eq!(parsed.points[1].nav, 3.42);
}

#[test]
fn lsjz_blocked_payload_parses_to_empty() {
    // 缺 Referer / 风控拦截形态：Data 是空字符串（非对象），宽容解析为空而非报错，
    // 但形态标记为 blocked——空表不可信（issue #1059，与「窗口内无新净值」区分）。
    let json = r#"{"Data":"","ErrCode":-999,"ErrMsg":"","TotalCount":0,"Expansion":null,"PageSize":0,"PageIndex":0}"#;
    let resp: NavResponse = serde_json::from_str(json).unwrap();
    let parsed = parse_lsjz(&resp);
    assert!(parsed.points.is_empty());
    assert!(parsed.blocked, "空响应形态必须标记，不得与正常空窗混同");
}

#[test]
fn lsjz_no_data_yet_shape_parses_to_empty() {
    // 查无此码 / 新基金未公布 / 增量窗口内无新净值：ErrCode=0、Data 是对象但
    // LSJZList 为空——空表可信（窗口内确实没有净值），不是被拦截形态。
    let json = r#"{"Data":{"LSJZList":[],"FundType":"","SYType":null,"isNewType":false,"Feature":null},"ErrCode":0,"ErrMsg":null,"TotalCount":0,"Expansion":null,"PageSize":20,"PageIndex":1}"#;
    let resp: NavResponse = serde_json::from_str(json).unwrap();
    let parsed = parse_lsjz(&resp);
    assert!(parsed.points.is_empty());
    assert!(!parsed.blocked, "正常空表不是空响应");
}

#[test]
fn lsjz_missing_data_field_is_blocked() {
    // Data 字段整体缺省（另一种空响应形态）：同样标记 blocked。
    let resp: NavResponse = serde_json::from_str(r#"{"ErrCode":-999,"TotalCount":0}"#).unwrap();
    let parsed = parse_lsjz(&resp);
    assert!(parsed.points.is_empty());
    assert!(parsed.blocked);
}

#[test]
fn lsjz_invalid_nav_rows_are_filtered() {
    // 未公布净值行（DWJZ 空串 / null / 0）静默过滤，与日线「无效样本不中断」同姿态。
    let json = r#"{"Data":{"LSJZList":[
        {"FSRQ":"2026-01-30","DWJZ":"1.2345"},
        {"FSRQ":"2026-01-29","DWJZ":""},
        {"FSRQ":"2026-01-28","DWJZ":null},
        {"FSRQ":"2026-01-27","DWJZ":0},
        {"FSRQ":"2026-01-26","DWJZ":2.5},
        {"FSRQ":"","DWJZ":1.5}
    ]},"TotalCount":6}"#;
    let resp: NavResponse = serde_json::from_str(json).unwrap();
    let points = parse_lsjz(&resp);
    assert_eq!(
        points.points,
        vec![
            crate::fund_nav::NavPoint {
                date: "2026-01-30".into(),
                nav: 1.2345
            },
            crate::fund_nav::NavPoint {
                date: "2026-01-26".into(),
                nav: 2.5
            },
        ]
    );
    assert!(!points.blocked);
}

// ---------------------------------------------------------------------------
// 货币基金口径（issue #1342）：货基的万份收益列不是单位净值——lsjz 响应自报
// 收益口径（SYType=每万份收益 / FundType=005）即判定，单位净值恒 1.0000，
// 只取收益日期。
// ---------------------------------------------------------------------------

/// 真实 lsjz 响应形状（货币基金 000905，实测 2026-09-15）：Data.FundType=005、
/// Data.SYType=每万份收益——DWJZ 列的 0.3117 是万份收益而非单位净值；0.0000
/// 是零收益日（周末归零前的真实形态），负值是货基偶发的负万份收益，null 是
/// 收益值缺省行——三者都不得影响日期收录。
const MONEY_FUND_PAYLOAD: &str = r#"{"Data":{"LSJZList":[{"FSRQ":"2026-09-14","DWJZ":"0.3117","LJJZ":"1.1410","SDATE":"","ACTUALSYI":"","NAVTYPE":"1","JZZZL":"0.00","SGZT":"限制大额申购","SHZT":"开放赎回","FHFCZ":"","FHFCZ10":"","FHFCBZ":"","DTYPE":null,"FHSP":""},{"FSRQ":"2026-09-13","DWJZ":"0.0000","LJJZ":"1.1410","SDATE":"","ACTUALSYI":"","NAVTYPE":"1","JZZZL":"0.00","SGZT":"限制大额申购","SHZT":"开放赎回","FHFCZ":"","FHFCZ10":"","FHFCBZ":"","DTYPE":null,"FHSP":""},{"FSRQ":"2026-09-12","DWJZ":"-0.6228","LJJZ":"1.1409","SDATE":"","ACTUALSYI":"","NAVTYPE":"1","JZZZL":"0.00","SGZT":"限制大额申购","SHZT":"开放赎回","FHFCZ":"","FHFCZ10":"","FHFCBZ":"","DTYPE":null,"FHSP":""},{"FSRQ":"2026-09-11","DWJZ":null,"LJJZ":"1.1410","SDATE":"","ACTUALSYI":"","NAVTYPE":"1","JZZZL":"0.00","SGZT":"限制大额申购","SHZT":"开放赎回","FHFCZ":"","FHFCZ10":"","FHFCBZ":"","DTYPE":null,"FHSP":""}],"FundType":"005","SYType":"每万份收益","isNewType":false,"Feature":null},"ErrCode":0,"ErrMsg":null,"TotalCount":3431,"Expansion":null,"PageSize":4,"PageIndex":1}"#;

#[test]
fn lsjz_money_fund_rows_pass_income_value_through_unrewritten() {
    // 判定不在此层（issue #1563 / ADR-0126 决策 3 换源）：东财自报口径退役后，
    // 取值列按事实解析、原样透传——货基行的 DWJZ（万份收益）不再被改写为恒定
    // 单位净值，落库由逐只刷新与历史首刷的官方披露判定门拦截（确认前不落任何
    // 取值，万份收益不得冒充单位净值，#1342）。零 / 负 / 缺省取值行按「无效行
    // 过滤」丢弃，与普通基金同姿态。
    let resp: NavResponse = serde_json::from_str(MONEY_FUND_PAYLOAD).unwrap();
    let parsed = parse_lsjz(&resp);
    assert!(!parsed.blocked);
    assert_eq!(
        parsed.points,
        vec![NavPoint {
            date: "2026-09-14".into(),
            nav: 0.3117
        }],
        "万份收益原样透传（不进价格：判定门拦在落库前）"
    );
}

#[test]
fn lsjz_unknown_fields_never_fail_response() {
    // 东财响应的未知 / 信号类字段（如退役前的 FundType/SYType，或未来新增字段）
    // 宽容忽略：整页照常解析、不中断同步——解析层只认消费的列。
    let json = r#"{"Data":{"LSJZList":[{"FSRQ":"2026-01-30","DWJZ":"3.3480"}],"FundType":5,"SYType":7},"TotalCount":1}"#;
    let resp: NavResponse = serde_json::from_str(json).unwrap();
    let parsed = parse_lsjz(&resp);
    assert_eq!(
        parsed.points,
        vec![NavPoint {
            date: "2026-01-30".into(),
            nav: 3.348
        }],
        "未知字段按普通基金口径解析原值"
    );
}

#[test]
fn nav_window_first_sync_backfills_two_years() {
    // 无水位（首刷）：起点 = 今天 − 2 年（恰为同月同日，无月末钳位），终点 = 今天。
    let (start, end) = nav_window(None, date("2026-08-29"));
    assert_eq!(start, "2024-08-29");
    assert_eq!(end, "2026-08-29");
}

#[test]
fn nav_window_incremental_starts_day_after_watermark() {
    // 有水位（现价缓存的净值日期）：从水位次日起，水位当日不重拉。
    let (start, end) = nav_window(Some("2026-01-30"), date("2026-08-29"));
    assert_eq!(start, "2026-01-31");
    assert_eq!(end, "2026-08-29");
}

#[test]
fn nav_window_illegal_watermark_falls_back_to_first_sync() {
    // 水位非法（理论不可达，写入侧恒 ISO 日期）：按首刷兜底自愈，不报错。
    let (start, end) = nav_window(Some("not-a-date"), date("2026-08-29"));
    assert_eq!(start, "2024-08-29");
    assert_eq!(end, "2026-08-29");
}

#[test]
fn nav_window_boundary_watermark_near_window_start() {
    // 水位早于两年窗口：起点取水位次日（增量语义不回看两年）。
    let (start, _) = nav_window(Some("2020-01-01"), date("2026-08-29"));
    assert_eq!(start, "2020-01-02");
}

// ---------------------------------------------------------------------------
// Referer 与页查询（本地 HTTP 服务验证头传播与报文组装，不依赖真实网络）
// ---------------------------------------------------------------------------

#[test]
fn nav_page_fetch_sends_referer_and_parses() {
    let (url, heads) = spawn_header_capture_server(REAL_PAYLOAD.to_string());
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let query = NavQuery {
        code: "110022".into(),
        start_date: "2024-08-30".into(),
        end_date: "2026-08-29".into(),
        page: 1,
    };
    let page = tauri::async_runtime::block_on(fetch_nav_page_from(
        &client,
        &mut pacer,
        &query,
        &[url.as_str()],
    ))
    .unwrap();

    // 报文组装：请求行携带 fundCode / pageIndex / pageSize / startDate / endDate。
    let head = &heads.lock().unwrap()[0];
    assert!(
        head.contains("GET /f10/lsjz?"),
        "请求路径应为 lsjz 接口: {head}"
    );
    assert!(head.contains("fundCode=110022"), "{head}");
    assert!(head.contains("pageIndex=1"), "{head}");
    assert!(head.contains("pageSize=20"), "{head}");
    assert!(head.contains("startDate=2024-08-30"), "{head}");
    assert!(head.contains("endDate=2026-08-29"), "{head}");
    // Referer 头（lsjz 接口的拦截前提）。
    assert!(
        head.to_lowercase()
            .contains("referer: http://fundf10.eastmoney.com/jjjz_110022.html"),
        "必须携带 f10 页面 Referer: {head}"
    );

    // 解析结果：净值点 + 顶层 TotalCount（分页循环定界依据）。
    assert_eq!(page.points.len(), 2);
    assert_eq!(page.total, 506);
}

#[test]
fn request_json_from_hosts_accepts_referer_argument() {
    // 泛型层 Referer 参数的传播（None 以外形状，供历史净值等接口复用）。
    let (url, heads) = spawn_header_capture_server(r#"{"ok":1}"#.to_string());
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let _: serde_json::Value = tauri::async_runtime::block_on(request_json_from_hosts(
        &client,
        &[("k", "v")],
        "/x",
        &[url.as_str()],
        crate::http::RetryConfig {
            max_retries: 0,
            base_backoff: Duration::ZERO,
            max_throttle_retries: 0,
            throttle_cooldown: Duration::ZERO,
        },
        &mut pacer,
        "test",
        Some("http://ref.example/"),
    ))
    .unwrap();
    let head = &heads.lock().unwrap()[0];
    assert!(
        head.to_lowercase().contains("referer: http://ref.example/"),
        "{head}"
    );
}
