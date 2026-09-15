//! 历史净值通道（issue #303 / ADR-0038 决策 6）：lsjz 报文解析（fixture 为真实
//! 接口形状）、净值同步水位窗口语义、Referer 头传播。全部离线驱动，不依赖真实
//! 网络；基金分区编排（水位增量回填的端到端语义）见 `instrument_info_sync.rs`。

use super::spawn_header_capture_server;
use std::time::Duration;

use chrono::NaiveDate;

use crate::fund_nav::{
    LsjzResponse, NavPoint, NavQuery, fetch_nav_full_series_from, fetch_nav_page_from, nav_window,
    parse_fund_archive, parse_lsjz, parse_money_fund_income_series, parse_net_worth_trend,
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
    let resp: LsjzResponse = serde_json::from_str(REAL_PAYLOAD).unwrap();
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
    let resp: LsjzResponse = serde_json::from_str(json).unwrap();
    let parsed = parse_lsjz(&resp);
    assert!(parsed.points.is_empty());
    assert!(parsed.blocked, "空响应形态必须标记，不得与正常空窗混同");
}

#[test]
fn lsjz_no_data_yet_shape_parses_to_empty() {
    // 查无此码 / 新基金未公布 / 增量窗口内无新净值：ErrCode=0、Data 是对象但
    // LSJZList 为空——空表可信（窗口内确实没有净值），不是被拦截形态。
    let json = r#"{"Data":{"LSJZList":[],"FundType":"","SYType":null,"isNewType":false,"Feature":null},"ErrCode":0,"ErrMsg":null,"TotalCount":0,"Expansion":null,"PageSize":20,"PageIndex":1}"#;
    let resp: LsjzResponse = serde_json::from_str(json).unwrap();
    let parsed = parse_lsjz(&resp);
    assert!(parsed.points.is_empty());
    assert!(!parsed.blocked, "正常空表不是空响应");
}

#[test]
fn lsjz_missing_data_field_is_blocked() {
    // Data 字段整体缺省（另一种空响应形态）：同样标记 blocked。
    let resp: LsjzResponse = serde_json::from_str(r#"{"ErrCode":-999,"TotalCount":0}"#).unwrap();
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
    let resp: LsjzResponse = serde_json::from_str(json).unwrap();
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
// 只取收益日期；档案文件缺单位净值序列而有万份收益序列是货基特征。
// ---------------------------------------------------------------------------

/// 真实 lsjz 响应形状（货币基金 000905，实测 2026-09-15）：Data.FundType=005、
/// Data.SYType=每万份收益——DWJZ 列的 0.3117 是万份收益而非单位净值；0.0000
/// 是零收益日（周末归零前的真实形态），负值是货基偶发的负万份收益，null 是
/// 收益值缺省行——三者都不得影响日期收录。
const MONEY_FUND_PAYLOAD: &str = r#"{"Data":{"LSJZList":[{"FSRQ":"2026-09-14","DWJZ":"0.3117","LJJZ":"1.1410","SDATE":"","ACTUALSYI":"","NAVTYPE":"1","JZZZL":"0.00","SGZT":"限制大额申购","SHZT":"开放赎回","FHFCZ":"","FHFCZ10":"","FHFCBZ":"","DTYPE":null,"FHSP":""},{"FSRQ":"2026-09-13","DWJZ":"0.0000","LJJZ":"1.1410","SDATE":"","ACTUALSYI":"","NAVTYPE":"1","JZZZL":"0.00","SGZT":"限制大额申购","SHZT":"开放赎回","FHFCZ":"","FHFCZ10":"","FHFCBZ":"","DTYPE":null,"FHSP":""},{"FSRQ":"2026-09-12","DWJZ":"-0.6228","LJJZ":"1.1409","SDATE":"","ACTUALSYI":"","NAVTYPE":"1","JZZZL":"0.00","SGZT":"限制大额申购","SHZT":"开放赎回","FHFCZ":"","FHFCZ10":"","FHFCBZ":"","DTYPE":null,"FHSP":""},{"FSRQ":"2026-09-11","DWJZ":null,"LJJZ":"1.1410","SDATE":"","ACTUALSYI":"","NAVTYPE":"1","JZZZL":"0.00","SGZT":"限制大额申购","SHZT":"开放赎回","FHFCZ":"","FHFCZ10":"","FHFCBZ":"","DTYPE":null,"FHSP":""}],"FundType":"005","SYType":"每万份收益","isNewType":false,"Feature":null},"ErrCode":0,"ErrMsg":null,"TotalCount":3431,"Expansion":null,"PageSize":4,"PageIndex":1}"#;

#[test]
fn lsjz_money_fund_income_column_normalizes_to_unit_nav() {
    // 货基判定命中：日期即净值日本体，单位净值恒 1.0000——万份收益数值（含 0
    // 与负值）甚至缺省（null）都不影响行有效性，也不进价格。
    let resp: LsjzResponse = serde_json::from_str(MONEY_FUND_PAYLOAD).unwrap();
    let parsed = parse_lsjz(&resp);
    assert!(!parsed.blocked);
    assert_eq!(
        parsed.points,
        vec![
            NavPoint {
                date: "2026-09-14".into(),
                nav: 1.0
            },
            NavPoint {
                date: "2026-09-13".into(),
                nav: 1.0
            },
            NavPoint {
                date: "2026-09-12".into(),
                nav: 1.0
            },
            NavPoint {
                date: "2026-09-11".into(),
                nav: 1.0
            },
        ]
    );
}

#[test]
fn lsjz_money_fund_signal_unknown_shape_never_fails_response() {
    // 判定信号（FundType/SYType）未知 wire 形态（如数字）：宽容归缺省，整页
    // 照常解析、不中断同步——信号缺席的代价是退回旧口径，不是报错。
    let json = r#"{"Data":{"LSJZList":[{"FSRQ":"2026-01-30","DWJZ":"3.3480"}],"FundType":5,"SYType":7},"TotalCount":1}"#;
    let resp: LsjzResponse = serde_json::from_str(json).unwrap();
    let parsed = parse_lsjz(&resp);
    assert_eq!(
        parsed.points,
        vec![NavPoint {
            date: "2026-01-30".into(),
            nav: 3.348
        }],
        "信号缺席按普通基金口径解析原值"
    );
}

#[test]
fn lsjz_money_fund_detected_by_fund_type_code_alone() {
    // SYType 缺省而 FundType=005：类型码单信号同样判定（两信号任一命中即真）。
    let json = r#"{"Data":{"LSJZList":[{"FSRQ":"2026-09-14","DWJZ":"0.3117"}],"FundType":"005","SYType":null},"TotalCount":1}"#;
    let resp: LsjzResponse = serde_json::from_str(json).unwrap();
    let parsed = parse_lsjz(&resp);
    assert_eq!(
        parsed.points,
        vec![NavPoint {
            date: "2026-09-14".into(),
            nav: 1.0
        }]
    );
}

/// 真实货币基金档案文件片段（000905，实测 2026-09-15）：没有单位净值序列变量
/// `Data_netWorthTrend`，万份收益序列 `Data_millionCopiesIncome` 为
/// `[北京时间午夜毫秒时间戳, 万份收益]` 升序数组。
const MONEY_FUND_ARCHIVE_JS: &str = r#"/*货币基金*/var ishb=true;var fS_name = "鹏华安盈宝货币A";var fS_code = "000905";
var Data_millionCopiesIncome = [[1694448000000,0.5807],[1694534400000,0.5809],[1694620800000,-0.5811]];
var Data_sevenDaysYearIncome = [[1694448000000,2.114],[1694534400000,2.115]];
"#;

#[test]
fn parse_money_fund_income_series_projects_income_dates_to_unit_nav() {
    // 收益序列的日期 × 恒定单位净值 1.0000；收益值（含负值）不消费；时间戳语义
    // 与单位净值序列一致（北京时间午夜毫秒 → 净值日期）。
    let points = parse_money_fund_income_series(MONEY_FUND_ARCHIVE_JS).unwrap();
    assert_eq!(
        points,
        vec![
            NavPoint {
                date: "2023-09-12".into(),
                nav: 1.0
            },
            NavPoint {
                date: "2023-09-13".into(),
                nav: 1.0
            },
            NavPoint {
                date: "2023-09-14".into(),
                nav: 1.0
            },
        ]
    );
}

#[test]
fn parse_money_fund_income_series_missing_or_malformed_is_none() {
    // 缺变量、整体不是数组、数组被截断、元素不是 [时间戳, 值] 对：都不可信，
    // 返回 None（与单位净值序列解析同姿态）。
    assert_eq!(parse_money_fund_income_series("var fS_name = \"x\";"), None);
    assert_eq!(
        parse_money_fund_income_series("var Data_millionCopiesIncome = 5;"),
        None
    );
    assert_eq!(
        parse_money_fund_income_series("var Data_millionCopiesIncome = [[1694448000000,0.58]"),
        None
    );
    assert_eq!(
        parse_money_fund_income_series("var Data_millionCopiesIncome = [[1694448000000]];"),
        None
    );
}

// ---------------------------------------------------------------------------
// 单请求全量净值通道（基金详情页数据文件，issue #1062）
// ---------------------------------------------------------------------------

/// 真实 pingzhongdata 数据文件片段（fundCode=110022，实测 2026-09-11）：单位净值
/// 序列变量 `Data_netWorthTrend` 为 `{x: 毫秒时间戳, y: 单位净值, ...}` 升序数组；
/// `Data_ACWorthTrend` 是并存的累计净值数组（本通道刻意不取）。片段同时带
/// 前缀声明与尾随声明，钉住「取变量、不误取另一数组」。
const REAL_PINGZHONG_SNIPPET: &str = r#"/*基金或股票信息*/var fS_name = "易方达消费行业股票";var fS_code = "110022";
var Data_ACWorthTrend = [[1282233600000,1.0],[1282838400000,1.001]];
var Data_netWorthTrend = [{"x":1282233600000,"y":1.0,"equityReturn":0,"unitMoney":""},{"x":1282838400000,"y":1.001,"equityReturn":0.1,"unitMoney":""},{"x":1283443200000,"y":1.006,"equityReturn":0.4995,"unitMoney":""}];
var Data_currentFundManager = [{"id":"1"}];
"#;

#[test]
fn parse_net_worth_trend_reads_real_fixture() {
    // x 为北京时间午夜的毫秒时间戳（UTC 前一日 16:00）：+8h 后取日期即净值日期。
    let points = parse_net_worth_trend(REAL_PINGZHONG_SNIPPET).unwrap();
    assert_eq!(
        points,
        vec![
            NavPoint {
                date: "2010-08-20".into(),
                nav: 1.0
            },
            NavPoint {
                date: "2010-08-27".into(),
                nav: 1.001
            },
            NavPoint {
                date: "2010-09-03".into(),
                nav: 1.006
            },
        ]
    );
}

#[test]
fn parse_net_worth_trend_skills_accumulated_array() {
    // 只有累计净值数组、没有单位净值数组：不得误取 Data_ACWorthTrend——返回 None
    // 让上层 fail-closed 回退分页通道（口径不一致比慢更糟）。
    let js = r#"var Data_ACWorthTrend = [[1282233600000,9.9],[1282838400000,9.8]];"#;
    assert_eq!(parse_net_worth_trend(js), None);
}

#[test]
fn parse_net_worth_trend_missing_or_malformed_is_none() {
    // 缺变量、整体不是数组、数组被截断：都无法作为可信数据源，返回 None。
    assert_eq!(parse_net_worth_trend("var fS_name = \"x\";"), None);
    assert_eq!(parse_net_worth_trend("var Data_netWorthTrend = 5;"), None);
    assert_eq!(
        parse_net_worth_trend(r#"var Data_netWorthTrend = [{"x":1282233600000,"y":1.0}"#),
        None
    );
}

#[test]
fn parse_net_worth_trend_well_formed_empty_is_some_empty() {
    // 结构完好但为空（新基金未公布净值）：是可信空结果，与「解析失败」区分开。
    assert_eq!(
        parse_net_worth_trend("var Data_netWorthTrend = [];"),
        Some(vec![])
    );
}

#[test]
fn parse_net_worth_trend_filters_invalid_rows() {
    // 单位净值 null / ≤0 的行静默过滤（与 lsjz 无效行同姿态），其余照常解析。
    let js = r#"var Data_netWorthTrend = [{"x":1282233600000,"y":1.0},{"x":1282838400000,"y":null},{"x":1283443200000,"y":0},{"x":1284048000000,"y":"1.5"}];"#;
    let points = parse_net_worth_trend(js).unwrap();
    assert_eq!(
        points,
        vec![
            NavPoint {
                date: "2010-08-20".into(),
                nav: 1.0
            },
            NavPoint {
                date: "2010-09-10".into(),
                nav: 1.5
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
        crate::http::RetryConfig {
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

#[test]
fn nav_full_series_fetch_reads_single_file() {
    // 单请求全量净值通道：一次 GET 详情页数据文件即取整只基金的历史净值序列
    // （issue #1062）。本地 HTTP 服务验证请求路径与报文解析，不依赖真实网络。
    let (url, heads) = spawn_header_capture_server(REAL_PINGZHONG_SNIPPET.to_string());
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let points =
        fetch_nav_full_series_from(&client, &mut pacer, "110022", &[url.as_str()]).unwrap();

    let head = &heads.lock().unwrap()[0];
    assert!(
        head.contains("GET /pingzhongdata/110022.js"),
        "请求路径应为基金详情页数据文件: {head}"
    );
    assert_eq!(
        points,
        vec![
            NavPoint {
                date: "2010-08-20".into(),
                nav: 1.0
            },
            NavPoint {
                date: "2010-08-27".into(),
                nav: 1.001
            },
            NavPoint {
                date: "2010-09-03".into(),
                nav: 1.006
            },
        ]
    );
}

#[test]
fn nav_full_series_fetch_untrusted_body_errors_for_fallback() {
    // 被风控拦截形态（HTML 而非数据文件）：解析不可信 → 返回 Err，上层 fail-closed
    // 回退分页通道，不把空结果当「无净值」静默吞掉。
    let (url, _) = spawn_header_capture_server("<html>blocked by waf</html>".to_string());
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    assert!(fetch_nav_full_series_from(&client, &mut pacer, "110022", &[url.as_str()]).is_err());
}

#[test]
fn nav_full_series_serves_money_fund_from_income_series() {
    // 货币基金没有单位净值序列：单请求通道直接按万份收益序列收录（日期 ×
    // 恒定单位净值 1.0000，issue #1342），不再必然失败回退分页通道。
    let (url, _) = spawn_header_capture_server(MONEY_FUND_ARCHIVE_JS.to_string());
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let points =
        fetch_nav_full_series_from(&client, &mut pacer, "000905", &[url.as_str()]).unwrap();
    assert_eq!(
        points,
        vec![
            NavPoint {
                date: "2023-09-12".into(),
                nav: 1.0
            },
            NavPoint {
                date: "2023-09-13".into(),
                nav: 1.0
            },
            NavPoint {
                date: "2023-09-14".into(),
                nav: 1.0
            },
        ]
    );
}

#[test]
fn nav_full_series_prefers_net_worth_trend_when_both_series_exist() {
    // 两序列并存的防御形态：单位净值序列优先——货基特征判定只在缺单位净值
    // 序列时生效，普通基金永不按 1.0000 收录。
    let both = format!(
        "{}\nvar Data_millionCopiesIncome = [[1694448000000,9.99]];",
        REAL_PINGZHONG_SNIPPET
    );
    let (url, _) = spawn_header_capture_server(both);
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let points =
        fetch_nav_full_series_from(&client, &mut pacer, "110022", &[url.as_str()]).unwrap();
    assert_eq!(points.len(), 3);
    assert_eq!(points[2].nav, 1.006, "取单位净值序列原值，不是 1.0 归一化");
}

// ---------------------------------------------------------------------------
// 档案通道解析（ADR-0039 修订，issue #1212）：同一份数据文件取权威名称与
// 最后一期单位净值；命中判据 = 代码全等 + 名称非空。
// ---------------------------------------------------------------------------

/// 已终止基金 002503 的档案文件真实形态截取（名称 / 代码变量 + 单位净值序列末两点）。
const ARCHIVE_JS: &str = r#"/*2023-11-19 00:21:59*/var ishb=false;var fS_name = "中银腾利混合C";var fS_code = "002503";
var Data_netWorthTrend = [{"x":1694448000000,"y":1.138,"equityReturn":-0.09,"unitMoney":""},{"x":1694966400000,"y":1.144,"equityReturn":0.62,"unitMoney":""}];
var Data_ACWorthTrend = [[1694966400000,1.415]];"#;

#[test]
fn parse_fund_archive_returns_name_and_last_nav() {
    let archive = parse_fund_archive(ARCHIVE_JS, "002503").expect("代码全等 + 名称非空应命中");
    assert_eq!(archive.name, "中银腾利混合C");
    let last = archive.last_nav.expect("应带回最后一期单位净值");
    assert_eq!(last.date, "2023-09-18", "取序列末点（净值日）");
    assert_eq!(last.nav, 1.144);
}

#[test]
fn parse_fund_archive_requires_code_equality_and_nonempty_name() {
    // 同码防守：文件声明的代码与请求代码不符不得命中（与搜索通道 FCODE 全等同纪律）。
    assert!(parse_fund_archive(ARCHIVE_JS, "003967").is_none());
    // 名称空：不可作为权威名称落库。
    let no_name = r#"var fS_name = "";var fS_code = "002503";"#;
    assert!(parse_fund_archive(no_name, "002503").is_none());
    // 无效代码被重定向到的错误页：没有声明变量，不命中。
    assert!(parse_fund_archive("<html>blocked by waf</html>", "002503").is_none());
    // 缺代码变量（形态不符）同样不命中。
    assert!(parse_fund_archive(r#"var fS_name = "某基金";"#, "002503").is_none());
}

#[test]
fn parse_fund_archive_degrades_to_name_only_without_nav_series() {
    // 名称齐备但净值序列缺失 / 不可信：按「未取到净值」降级（与搜索通道
    // 「命中但未公布净值」同形），名称仍可用于建行。
    let js = r#"var fS_name = "某基金";var fS_code = "110022";"#;
    let archive = parse_fund_archive(js, "110022").expect("名称齐备即命中");
    assert_eq!(archive.name, "某基金");
    assert!(archive.last_nav.is_none());
}

#[test]
fn parse_fund_archive_money_fund_last_nav_is_unit_nav_on_latest_income_date() {
    // 货基没有单位净值序列：最后一期净值 = 最新收益日 × 恒定单位净值 1.0000
    //（issue #1342），档案回退报价据此落 1.0000 而非无价或万份收益。
    let archive = parse_fund_archive(MONEY_FUND_ARCHIVE_JS, "000905").expect("代码全等应命中");
    assert_eq!(archive.name, "鹏华安盈宝货币A");
    let last = archive.last_nav.expect("货基按收益序列取最后一期净值");
    assert_eq!(last.date, "2023-09-14");
    assert_eq!(last.nav, 1.0);
}
