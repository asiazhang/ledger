//! 历史净值通道（issue #303 / ADR-0038 决策 6）：lsjz 报文解析（fixture 为真实
//! 接口形状）、净值同步水位窗口语义、Referer 头传播。全部离线驱动，不依赖真实
//! 网络；基金分区编排（水位增量回填的端到端语义）见 `holding_price_sync.rs`。

use std::sync::Arc;
use std::time::Duration;

use chrono::NaiveDate;

use crate::sync::fund_nav::{LsjzResponse, NavQuery, fetch_nav_page_from, nav_window, parse_lsjz};
use crate::sync::http::{Pacer, request_json_from_hosts};

/// 真实 lsjz 响应形状（fundCode=110022，实测 2026-08）：Data.LSJZList 按净值
/// 日期降序，DWJZ 为数字字符串，TotalCount 在顶层。
const REAL_PAYLOAD: &str = r#"{"Data":{"LSJZList":[{"FSRQ":"2026-01-30","DWJZ":"3.3480","LJJZ":"3.3480","SDATE":null,"ACTUALSYI":"","NAVTYPE":"1","JZZZL":"-2.11","SGZT":"开放申购","SHZT":"开放赎回","FHFCZ":"","FHFCZ10":null,"FHFCBZ":"","DTYPE":null,"FHSP":""},{"FSRQ":"2026-01-29","DWJZ":"3.4200","LJJZ":"3.4200","SDATE":null,"ACTUALSYI":"","NAVTYPE":"1","JZZZL":"3.86","SGZT":"开放申购","SHZT":"开放赎回","FHFCZ":"","FHFCZ10":null,"FHFCBZ":"","DTYPE":null,"FHSP":""}],"FundType":"001","SYType":null,"isNewType":false,"Feature":null},"ErrCode":0,"ErrMsg":null,"TotalCount":506,"Expansion":null,"PageSize":5,"PageIndex":1}"#;

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[test]
fn lsjz_response_deserializes_real_payload() {
    let resp: LsjzResponse = serde_json::from_str(REAL_PAYLOAD).unwrap();
    assert_eq!(resp.total_count, 506);
    let points = parse_lsjz(&resp);
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].date, "2026-01-30");
    assert_eq!(points[0].nav, 3.3480);
    assert_eq!(points[1].date, "2026-01-29");
    assert_eq!(points[1].nav, 3.42);
}

#[test]
fn lsjz_blocked_payload_parses_to_empty() {
    // 缺 Referer / 风控拦截形态：Data 是空字符串（非对象），宽容解析为空而非报错。
    let json = r#"{"Data":"","ErrCode":-999,"ErrMsg":"","TotalCount":0,"Expansion":null,"PageSize":0,"PageIndex":0}"#;
    let resp: LsjzResponse = serde_json::from_str(json).unwrap();
    assert!(parse_lsjz(&resp).is_empty());
}

#[test]
fn lsjz_no_data_yet_shape_parses_to_empty() {
    // 查无此码 / 新基金未公布：ErrCode=0 但 LSJZList 为空。
    let json = r#"{"Data":{"LSJZList":[],"FundType":"","SYType":null,"isNewType":false,"Feature":null},"ErrCode":0,"ErrMsg":null,"TotalCount":0,"Expansion":null,"PageSize":20,"PageIndex":1}"#;
    let resp: LsjzResponse = serde_json::from_str(json).unwrap();
    assert!(parse_lsjz(&resp).is_empty());
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
    let resp: LsjzResponse = serde_json::from_str(json).unwrap();
    let points = parse_lsjz(&resp);
    assert_eq!(
        points,
        vec![
            crate::sync::fund_nav::NavPoint {
                date: "2026-01-30".into(),
                nav: 1.2345
            },
            crate::sync::fund_nav::NavPoint {
                date: "2026-01-26".into(),
                nav: 2.5
            },
        ]
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

/// 起一个捕获请求头的本地 HTTP 服务，返回 (基础地址, 请求头收集器)。
fn spawn_header_capture_server(body: String) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let heads = Arc::new(std::sync::Mutex::new(Vec::new()));
    let heads_clone = heads.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            heads_clone
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf).to_string());
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    (url, heads)
}

#[test]
fn nav_page_fetch_sends_referer_and_parses() {
    let (url, heads) = spawn_header_capture_server(REAL_PAYLOAD.to_string());
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let query = NavQuery {
        code: "110022".into(),
        start_date: "2024-08-30".into(),
        end_date: "2026-08-29".into(),
        page: 1,
    };
    let page = fetch_nav_page_from(&client, &mut pacer, &query, &[url.as_str()]).unwrap();

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
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let _: serde_json::Value = request_json_from_hosts(
        &client,
        &[("k", "v")],
        "/x",
        &[url.as_str()],
        crate::sync::http::RetryConfig {
            max_retries: 0,
            base_backoff: Duration::ZERO,
            max_throttle_retries: 0,
            throttle_cooldown: Duration::ZERO,
        },
        &mut pacer,
        "test",
        Some("http://ref.example/"),
    )
    .unwrap();
    let head = &heads.lock().unwrap()[0];
    assert!(
        head.to_lowercase().contains("referer: http://ref.example/"),
        "{head}"
    );
}
