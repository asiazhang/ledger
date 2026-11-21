use std::time::Duration;

use crate::bulk::BulkNavPoint;
use crate::http::Pacer;
use crate::sina_fund::{
    SINA_FUND_BATCH_HOSTS, SINA_FUND_BATCH_PATH_PREFIX, SINA_FUND_BATCH_REFERER,
    SINA_FUND_BATCH_SIZE, SINA_FUND_HISTORY_HOSTS, SinaFundNavForm, fetch_fund_nav_history,
    fetch_sina_fund_nav_rows, fund_batch_from_rows, parse_fund_nav_history,
    parse_sina_fund_nav_rows,
};

const BATCH_BODY: &str = "\
var hq_str_f_000001=\"华夏成长混合A,1.333,3.906,1.298,2026-09-18,24.0598\";
var hq_str_f_000198=\"天弘余额宝货币,0.2229,0.824,,2026-09-19,6799.46\";
var hq_str_f_002503=\"中银腾利混合C,1.144,1.415,1.137,2023-09-18,0.071287\";
var hq_str_f_000659=\"银华活钱宝货币C,,1.233,,2015-01-16,0\";
var hq_str_f_999999=\"\";
";

const HISTORY_BODY_NORMAL: &str = r#"{"result":{"status":{"code":0},"data":{"data":[{"fbrq":"2026-09-18 00:00:00","jjjz":"1.333","ljjz":"3.906"},{"fbrq":"2026-09-17 00:00:00","jjjz":"1.298","ljjz":"3.871"},{"fbrq":"2026-09-16 00:00:00","jjjz":"1.296","ljjz":"3.869"}],"total_num":"3"}}}"#;

const HISTORY_BODY_TERMINATED: &str = r#"{"result":{"status":{"code":0},"data":{"data":[{"fbrq":"2023-09-18 00:00:00","jjjz":"1.144","ljjz":"1.415"},{"fbrq":"2023-09-15 00:00:00","jjjz":"1.137","ljjz":"1.408"},{"fbrq":"2023-09-14 00:00:00","jjjz":"1.138","ljjz":"1.409"}],"total_num":"3"}}}"#;

const HISTORY_BODY_EMPTY: &str =
    r#"{"result":{"status":{"code":0},"data":{"data":[],"total_num":"0"}}}"#;

fn parse_batch(body: &str) -> Vec<crate::sina_fund::SinaFundNavRow> {
    parse_sina_fund_nav_rows(body).expect("真实报文应可解析")
}

#[test]
fn batch_fixture_pins_row_forms_names_and_dates() {
    let rows = parse_batch(BATCH_BODY);
    assert_eq!(rows.len(), 4, "查无此码的空值语句不产出行");

    let normal = &rows[0];
    assert_eq!(normal.code, "000001");
    assert_eq!(normal.name, "华夏成长混合A");
    assert_eq!(normal.nav_date, "2026-09-18");
    assert_eq!(
        normal.form,
        SinaFundNavForm::UnitNav {
            unit_nav: 1.333,
            accumulated_nav: Some(3.906),
            prev_unit_nav: Some(1.298),
        }
    );

    let money = &rows[1];
    assert_eq!(money.code, "000198");
    assert_eq!(money.name, "天弘余额宝货币");
    assert_eq!(money.nav_date, "2026-09-19");
    assert_eq!(
        money.form,
        SinaFundNavForm::MoneyYield {
            gain_per_wan: Some(0.2229),
            seven_day_annual_percent: Some(0.824),
        }
    );

    let terminated = &rows[2];
    assert_eq!(terminated.code, "002503");
    assert_eq!(
        terminated.form,
        SinaFundNavForm::UnitNav {
            unit_nav: 1.144,
            accumulated_nav: Some(1.415),
            prev_unit_nav: Some(1.137),
        }
    );
    assert_eq!(terminated.nav_date, "2023-09-18");

    let terminated_money = &rows[3];
    assert_eq!(terminated_money.code, "000659");
    assert_eq!(
        terminated_money.form,
        SinaFundNavForm::MoneyYield {
            gain_per_wan: None,
            seven_day_annual_percent: Some(1.233),
        }
    );
}

#[test]
fn money_fund_misalignment_is_never_trusted_as_unit_nav() {
    let rows = parse_batch(BATCH_BODY);
    let money = rows
        .iter()
        .find(|row| row.code == "000198")
        .expect("货基行应在场")
        .clone();
    assert!(money.into_nav_point().is_none(), "货基错位行不产出价格点");

    let money_only = parse_batch(
        "var hq_str_f_000198=\"天弘余额宝货币,0.2229,0.824,,2026-09-19,6799.46\";\nvar hq_str_f_000659=\"银华活钱宝货币C,,1.233,,2015-01-16,0\";\nvar hq_str_f_000658=\"负收益货基,-0.1234,0.5,,2026-09-19,10\";\n",
    );
    assert_eq!(
        money_only[2].form,
        SinaFundNavForm::MoneyYield {
            gain_per_wan: Some(-0.1234),
            seven_day_annual_percent: Some(0.5)
        }
    );
    let points: Vec<Option<BulkNavPoint>> = money_only
        .into_iter()
        .map(|row| row.into_nav_point())
        .collect();
    assert_eq!(points, vec![None, None, None]);
}

#[test]
fn into_nav_point_projects_unit_nav_rows_only() {
    let rows = parse_batch(BATCH_BODY);
    let normal = rows
        .iter()
        .find(|row| row.code == "000001")
        .expect("普通行应在场")
        .clone();
    assert_eq!(
        normal.into_nav_point(),
        Some(BulkNavPoint {
            date: "2026-09-18".into(),
            nav: 1.333,
        }),
        "普通行投影「净值日期 + 单位净值」"
    );
}

#[test]
fn fund_batch_from_rows_splits_names_from_price_points() {
    let batch = fund_batch_from_rows(parse_batch(BATCH_BODY));
    assert!(batch.covers("000001"));
    assert!(batch.covers("000198"), "货基行被面收录（有名称）");
    assert_eq!(batch.name_of("000198"), Some("天弘余额宝货币"));
    assert!(batch.nav_of("000198").is_none(), "货基行不产出价格点");
    assert_eq!(
        batch.nav_of("000001"),
        Some(&BulkNavPoint {
            date: "2026-09-18".into(),
            nav: 1.333,
        })
    );
    assert!(batch.nav_of("002503").is_some(), "已终止普通基金末点照常在");
    assert!(!batch.covers("999999"), "查无此码的空值语句不产出行");
    assert_eq!(batch.names.len(), 4, "四条非空语句进名称字典");
    assert_eq!(batch.nav.len(), 2, "只有两条普通行产出价格点");
}

#[test]
fn invalid_batch_rows_are_skipped_as_gaps_not_failures() {
    let body = "\
var hq_str_f_000001=\"华夏成长混合A,1.333,3.906,1.298,2026-09-18,24.0598\";
var hq_str_f_000002=\",1.0,1.0,1.0,2026-09-18,1\";
var hq_str_f_000003=\"短字段,1.0,1.0,1.0\";
var hq_str_f_000004=\"无日期,1.0,1.0,1.0,,1\";
var hq_str_f_000005=\"零净值,0,1.0,1.0,2026-09-18,1\";
var hq_str_f_000006=\"负净值,-1.5,1.0,1.0,2026-09-18,1\";
var hq_str_f_999999=\"\";
";
    let rows = parse_batch(body);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].code, "000001");

    assert!(parse_batch("var hq_str_f_999999=\"\";\nvar hq_str_f_888888=\"\";\n").is_empty());
}

#[test]
fn intercepted_batch_response_fails_closed() {
    for body in [
        "",
        "   \n\t",
        "Forbidden",
        "<html><body>risk control</body></html>",
        "ErrCode=-999",
    ] {
        assert!(
            parse_sina_fund_nav_rows(body).is_err(),
            "应 fail-closed：{body:?}"
        );
    }
}

#[test]
fn batch_statement_scanner_tolerates_line_shapes() {
    let body = concat!(
        "var hq_str_f_000001=\"华夏成长混合A,1.333,3.906,1.298,2026-09-18,24.0598\";",
        "var hq_str_f_000198=\"天弘余额宝货币,0.2229,0.824,,2026-09-19,6799.46\";",
    );
    assert_eq!(parse_batch(body).len(), 2);
    let no_var = "hq_str_f_000001=\"华夏成长混合A,1.333,3.906,1.298,2026-09-18,24.0598\";\n";
    assert_eq!(parse_batch(no_var).len(), 1);
}

#[test]
fn history_fixture_reaches_last_point_of_terminated_fund() {
    let points = parse_fund_nav_history(HISTORY_BODY_TERMINATED).expect("已终止基金应可解析");
    assert_eq!(points.len(), 3);
    let last = points
        .iter()
        .max_by(|a, b| a.date.cmp(&b.date))
        .expect("末点在场");
    assert_eq!(last.date, "2023-09-18");
    assert_eq!(last.nav, 1.144);

    let points = parse_fund_nav_history(HISTORY_BODY_NORMAL).expect("普通基金应可解析");
    assert_eq!(points.len(), 3);
    assert_eq!(points[0].date, "2026-09-18");
    assert_eq!(points[0].nav, 1.333);
    assert_eq!(points[2].date, "2026-09-16");
    assert_eq!(points[2].nav, 1.296);
}

#[test]
fn history_empty_payload_is_trusted_empty() {
    assert!(
        parse_fund_nav_history(HISTORY_BODY_EMPTY)
            .expect("可信空应返回空序列")
            .is_empty()
    );
}

#[test]
fn history_row_discipline_filters_invalid_rows() {
    let body = r#"{"result":{"status":{"code":0},"data":{"data":[
        {"fbrq":"2026-09-18 00:00:00","jjjz":"1.333","ljjz":"3.906"},
        {"fbrq":"","jjjz":"1.0"},
        {"fbrq":"not-a-date","jjjz":"1.0"},
        {"fbrq":"2026-09-17 00:00:00","jjjz":"0"},
        {"fbrq":"2026-09-16 00:00:00","jjjz":"-1.0"}
    ],"total_num":"1"}}}"#;
    let points = parse_fund_nav_history(body).expect("有有效行即产出");
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].date, "2026-09-18");

    let body = r#"{"result":{"status":{"code":0},"data":{"data":[
        {"fbrq":"2026-09-18 00:00:00","jjjz":"1.333"},
        {"fbrq":"","jjjz":"1.0"},
        {"fbrq":"not-a-date","jjjz":"1.0"},
        {"fbrq":"2026-09-17 00:00:00","jjjz":"0"},
        {"fbrq":"2026-09-16 00:00:00","jjjz":"-1.0"}
    ],"total_num":"5"}}}"#;
    let points = parse_fund_nav_history(body).expect("原始行数取全即完整");
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].date, "2026-09-18");
}

#[test]
fn history_untrusted_shapes_fail_closed() {
    for body in [
        "<html><body>risk control</body></html>",
        "",
        "{}",
        r#"{"result":null}"#,
        r#"{"result":{"status":{"code":0}}}"#,
        r#"{"result":{"status":{"code":0},"data":{}}}"#,
        r#"{"result":{"status":{"code":-1},"data":{"data":[],"total_num":"0"}}}"#,
        r#"{"result":{"status":{"code":0},"data":{"data":[{"fbrq":"2026-09-18 00:00:00","jjjz":"1.333"}],"total_num":"6010"}}}"#,
        r#"{"result":{"status":{"code":0},"data":{"data":[{"fbrq":"2026-09-18 00:00:00","jjjz":"x"}],"total_num":"1"}}}"#,
    ] {
        assert!(
            parse_fund_nav_history(body).is_err(),
            "应 fail-closed：{body:?}"
        );
    }
}

#[test]
fn fetch_pins_request_shape_referer_and_batch_capacity() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let gbk = encoding_rs::GBK.encode(BATCH_BODY).0.into_owned();

    let codes: Vec<String> = ["000001", "000198", "002503", "999999"]
        .iter()
        .map(|c| c.to_string())
        .collect();
    let (url, requests) = crate::tests::spawn_header_capture_server(gbk.clone());
    let rows = tauri::async_runtime::block_on(fetch_sina_fund_nav_rows(
        &client,
        &mut pacer,
        &[url.as_str()],
        &codes,
    ))
    .expect("假响应应解析成功");
    assert_eq!(rows.len(), 4, "假响应含四只非空行");
    let captured = requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 1, "一批一次请求");
    assert_eq!(
        request_target(&captured[0]),
        "/list=f_000001,f_000198,f_002503,f_999999",
        "请求形态：路径段 list=f_<代码逗号串>"
    );
    assert!(
        captured[0].to_ascii_lowercase().contains(&format!(
            "referer: {}",
            SINA_FUND_BATCH_REFERER.to_ascii_lowercase()
        )),
        "批量面必须携带 Referer（缺省 403）：{}",
        captured[0]
    );

    let many: Vec<String> = (0..SINA_FUND_BATCH_SIZE + 1)
        .map(|i| format!("{i:06}"))
        .collect();
    let (url, requests) = crate::tests::spawn_header_capture_server(gbk);
    let rows = tauri::async_runtime::block_on(fetch_sina_fund_nav_rows(
        &client,
        &mut pacer,
        &[url.as_str()],
        &many,
    ))
    .expect("分批请求应成功");
    assert_eq!(rows.len(), 8, "两批各回同一份 fixture 的四行");
    let captured = requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 2, "超过承载量应分两次请求");
    assert_eq!(
        key_count(request_target(&captured[0])),
        SINA_FUND_BATCH_SIZE
    );
    assert_eq!(key_count(request_target(&captured[1])), 1);
    let worst_request_line = SINA_FUND_BATCH_PATH_PREFIX.len() + SINA_FUND_BATCH_SIZE * 10;
    assert!(
        worst_request_line < 8_100,
        "承载量最坏请求行 {worst_request_line} 应低于实测 431 触发线（~8.1 KB）"
    );
}

#[test]
fn fetch_batch_decodes_gbk_and_fails_closed_on_intercepted_response() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let codes = vec!["000001".to_string()];

    let gbk = encoding_rs::GBK.encode(BATCH_BODY).0.into_owned();
    let (url, _) = crate::tests::spawn_header_capture_server(gbk);
    let rows = tauri::async_runtime::block_on(fetch_sina_fund_nav_rows(
        &client,
        &mut pacer,
        &[url.as_str()],
        &codes,
    ))
    .expect("GBK 报文应解析成功");
    assert_eq!(rows[0].name, "华夏成长混合A");

    for body in [
        b"<html><body>risk control</body></html>".to_vec(),
        vec![0xff, 0xfe, 0x00, 0x01],
    ] {
        let (url, _) = crate::tests::spawn_header_capture_server(body);
        let error = tauri::async_runtime::block_on(fetch_sina_fund_nav_rows(
            &client,
            &mut pacer,
            &[url.as_str()],
            &codes,
        ))
        .expect_err("被拦截响应应报错");
        assert!(
            error.is_code("sync.fund-batch-source-malformed"),
            "被拦截响应应报批量源不可信码化错误，实际 {error:?}"
        );
    }
}

#[test]
fn fetch_pins_history_request_shape() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (url, requests) = crate::tests::spawn_header_capture_server(HISTORY_BODY_TERMINATED);

    let points = tauri::async_runtime::block_on(fetch_fund_nav_history(
        &client,
        &mut pacer,
        &[url.as_str()],
        "002503",
        None,
        None,
    ))
    .expect("全历史假响应应解析成功");
    assert_eq!(
        points
            .iter()
            .max_by(|a, b| a.date.cmp(&b.date))
            .expect("末点在场")
            .date,
        "2023-09-18"
    );

    let captured = requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 1, "单只一次请求取全");
    let target = request_target(&captured[0]);
    assert!(
        target.starts_with("/fundInfo/api/openapi.php/CaihuiFundInfoService.getNav?"),
        "请求目标应是全历史接口路径：{target}"
    );
    for expected in [
        "symbol=002503",
        "datefrom=",
        "dateto=",
        "page=1",
        "num=10000",
    ] {
        assert!(target.contains(expected), "请求参数缺 {expected}：{target}");
    }
}

#[test]
fn production_hosts_and_path_pin_to_sina_fund_endpoints() {
    assert_eq!(SINA_FUND_BATCH_HOSTS, ["https://hq.sinajs.cn"]);
    assert_eq!(SINA_FUND_BATCH_PATH_PREFIX, "/list=");
    assert_eq!(SINA_FUND_BATCH_REFERER, "https://finance.sina.com.cn/");
    assert_eq!(
        SINA_FUND_HISTORY_HOSTS,
        ["https://stock.finance.sina.com.cn"]
    );
}

fn request_target(head: &str) -> &str {
    head.lines()
        .next()
        .unwrap_or("")
        .split(' ')
        .nth(1)
        .unwrap_or("")
}

fn key_count(target: &str) -> usize {
    let keys = target
        .strip_prefix(SINA_FUND_BATCH_PATH_PREFIX)
        .unwrap_or(target);
    if keys.is_empty() {
        0
    } else {
        keys.split(',').count()
    }
}
