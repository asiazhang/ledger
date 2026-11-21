use std::time::Duration;

use crate::fund::fetch_fund_quote_from;
use crate::http::Pacer;
use crate::sina_fund::SINA_FUND_BATCH_REFERER;
use ledger_infra::error::AppError;
use ledger_investment::prices::{CSRC_PRICE_SOURCE, SINA_PRICE_SOURCE};

const BATCH_BODY: &str = concat!(
    "var hq_str_f_000001=\"华夏成长混合A,1.333,1.552,1.329,2026-09-18,12.3456\";\n",
    "var hq_str_f_000198=\"天弘余额宝货币,0.2229,0.824,,2026-09-19,6799.46\";\n",
    "var hq_str_f_002503=\"\";\n",
);

const CSRC_MONEY_PAYLOAD: &str = r#"{"sEcho":1,"iTotalRecords":1,"iTotalDisplayRecords":1,"aaData":[
{"code":"000198","shortName":"天弘余额宝货币","shareNetValue":"","totalNetValue":"","valuationDate":"2026-09-18","gainPer":"0.2274","yearSevenDayYieldRatePercent":"0.8230%"}]}"#;

const CSRC_TERMINATED_MONEY_PAYLOAD: &str = r#"{"sEcho":1,"iTotalRecords":1,"iTotalDisplayRecords":1,"aaData":[
{"code":"000905","shortName":"鹏华安盈宝货币A","shareNetValue":"","totalNetValue":"","valuationDate":"2024-01-05","gainPer":"0.3123","yearSevenDayYieldRatePercent":"1.1710%"}]}"#;

const CSRC_TERMINATED_PAYLOAD: &str = r#"{"sEcho":1,"iTotalRecords":2,"iTotalDisplayRecords":2,"aaData":[
{"code":"002503","shortName":"中银腾利混合C","shareNetValue":"1.144","totalNetValue":"1.415","valuationDate":"2023-09-18","gainPer":"","yearSevenDayYieldRatePercent":""},
{"code":"002503","shortName":"中银腾利混合C","shareNetValue":"1.137","totalNetValue":"1.408","valuationDate":"2023-09-15","gainPer":"","yearSevenDayYieldRatePercent":""}]}"#;

const CSRC_TRUSTED_EMPTY_PAYLOAD: &str =
    r#"{"sEcho":1,"iTotalRecords":0,"iTotalDisplayRecords":0,"aaData":[]}"#;

const CSRC_ERROR_HTML: &str =
    r#"<html xmlns="http://www.w3.org/1999/xh"><head><title>系统异常</title></head></html>"#;

fn gbk(body: &str) -> Vec<u8> {
    encoding_rs::GBK.encode(body).0.into_owned()
}

fn request_line(head: &str) -> &str {
    head.lines().next().unwrap_or_default()
}

#[test]
fn normal_fund_answers_from_batch_face_alone() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (sina, sina_requests) = crate::tests::spawn_header_capture_server(gbk(BATCH_BODY));
    let (csrc, csrc_requests) =
        crate::tests::spawn_header_capture_server(CSRC_MONEY_PAYLOAD.to_string());

    let quote = tauri::async_runtime::block_on(fetch_fund_quote_from(
        &client,
        &mut pacer,
        "000001",
        &[sina.as_str()],
        &[csrc.as_str()],
    ))
    .expect("普通行应命中批量面");

    assert_eq!(quote.code, "000001");
    assert_eq!(quote.name, "华夏成长混合A");
    assert_eq!(quote.price_cents, Some(13_330));
    assert_eq!(quote.nav_date.as_deref(), Some("2026-09-18"));
    assert_eq!(quote.price_date.as_deref(), Some("2026-09-18"));
    assert_eq!(quote.fund_class, None);
    assert_eq!(quote.constant_unit_price_cents, None);
    assert_eq!(quote.price_source, SINA_PRICE_SOURCE);

    let captured = sina_requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 1, "批量面一次请求");
    assert!(
        request_line(&captured[0]).contains("/list=f_000001"),
        "批量面请求形态：{}",
        captured[0]
    );
    assert!(
        captured[0]
            .to_ascii_lowercase()
            .contains(&format!("referer: {SINA_FUND_BATCH_REFERER}")),
        "批量面必须携带 Referer：{}",
        captured[0]
    );
    assert!(
        csrc_requests.lock().unwrap().is_empty(),
        "普通行不应触发官方披露请求"
    );
}

#[test]
fn money_fund_row_confirmed_by_disclosure_lands_constant_price() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (sina, _) = crate::tests::spawn_header_capture_server(gbk(BATCH_BODY));
    let (csrc, csrc_requests) =
        crate::tests::spawn_header_capture_server(CSRC_MONEY_PAYLOAD.to_string());

    let quote = tauri::async_runtime::block_on(fetch_fund_quote_from(
        &client,
        &mut pacer,
        "000198",
        &[sina.as_str()],
        &[csrc.as_str()],
    ))
    .expect("货基错位行应经官方披露确认");

    assert_eq!(quote.name, "天弘余额宝货币");
    assert_eq!(quote.price_cents, Some(10_000));
    assert_eq!(quote.constant_unit_price_cents, Some(10_000));
    assert_eq!(quote.nav_date.as_deref(), Some("2026-09-19"));
    assert_eq!(quote.price_source, CSRC_PRICE_SOURCE);

    let captured = csrc_requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 1, "判定确认一次请求");
    assert!(
        request_line(&captured[0]).contains("000198"),
        "官方披露请求应携带基金代码：{}",
        captured[0]
    );
}

#[test]
fn terminated_fund_missing_from_batch_falls_back_to_disclosure() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (sina, _) = crate::tests::spawn_header_capture_server(gbk(BATCH_BODY));
    let (csrc, csrc_requests) =
        crate::tests::spawn_header_capture_server(CSRC_TERMINATED_PAYLOAD.to_string());

    let quote = tauri::async_runtime::block_on(fetch_fund_quote_from(
        &client,
        &mut pacer,
        "002503",
        &[sina.as_str()],
        &[csrc.as_str()],
    ))
    .expect("已终止基金应由官方披露兜底识别");

    assert_eq!(quote.code, "002503");
    assert_eq!(quote.name, "中银腾利混合C");
    assert_eq!(quote.price_cents, Some(11_440));
    assert_eq!(quote.nav_date.as_deref(), Some("2023-09-18"));
    assert_eq!(quote.price_source, CSRC_PRICE_SOURCE);

    let captured = csrc_requests.lock().unwrap().clone();
    assert_eq!(
        captured.len(),
        1,
        "兜底区间查询一次请求（aoData 编码串内含 fundCode）"
    );
    assert!(
        request_line(&captured[0]).contains("002503"),
        "兜底请求应携带基金代码：{}",
        captured[0]
    );
}

#[test]
fn terminated_money_fund_row_confirms_constant_price_too() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let terminated_money_batch = "var hq_str_f_000905=\"鹏华安盈宝货币A,,, ,2024-01-05,\";\n";
    let (sina, _) = crate::tests::spawn_header_capture_server(gbk(terminated_money_batch));
    let (csrc, _) =
        crate::tests::spawn_header_capture_server(CSRC_TERMINATED_MONEY_PAYLOAD.to_string());

    let quote = tauri::async_runtime::block_on(fetch_fund_quote_from(
        &client,
        &mut pacer,
        "000905",
        &[sina.as_str()],
        &[csrc.as_str()],
    ))
    .expect("已终止货基应经判定确认落恒定价");
    assert_eq!(quote.name, "鹏华安盈宝货币A");
    assert_eq!(quote.price_cents, Some(10_000));
    assert_eq!(quote.constant_unit_price_cents, Some(10_000));
}

#[test]
fn missing_from_both_sources_is_coded_not_found() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (sina, _) = crate::tests::spawn_header_capture_server(gbk(BATCH_BODY));
    let (csrc, _) =
        crate::tests::spawn_header_capture_server(CSRC_TRUSTED_EMPTY_PAYLOAD.to_string());

    let err = tauri::async_runtime::block_on(fetch_fund_quote_from(
        &client,
        &mut pacer,
        "999999",
        &[sina.as_str()],
        &[csrc.as_str()],
    ))
    .expect_err("两源皆未收录应查无此码");
    assert!(
        err.is_code("sync.fund-not-found"),
        "查无此码应保持既有码化错误，实际 {err:?}"
    );
}

#[test]
fn fund_not_found_predicate_recognizes_constructor_error() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (sina, _) = crate::tests::spawn_header_capture_server(gbk(BATCH_BODY));
    let (csrc, _) =
        crate::tests::spawn_header_capture_server(CSRC_TRUSTED_EMPTY_PAYLOAD.to_string());

    let err = tauri::async_runtime::block_on(fetch_fund_quote_from(
        &client,
        &mut pacer,
        "999999",
        &[sina.as_str()],
        &[csrc.as_str()],
    ))
    .expect_err("两源皆未收录应查无此码");
    assert!(
        crate::fund::is_fund_not_found(&err),
        "构造器产出的查无此码应被谓词识别，实际 {err:?}"
    );

    let other_code = AppError::coded("sync.disclosure-source-malformed", "稍后再试");
    assert!(
        !crate::fund::is_fund_not_found(&other_code),
        "非查无此码的码化错误不得被谓词识别"
    );
    let io = AppError::Io("HTTP 请求失败: 连接超时".into());
    assert!(
        !crate::fund::is_fund_not_found(&io),
        "网络类失败不得被谓词识别（否则名称刷新降级会吞掉可重试故障）"
    );
}

#[test]
fn disclosure_malformed_propagates_fail_closed() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (sina, _) = crate::tests::spawn_header_capture_server(gbk(BATCH_BODY));
    let (csrc, _) = crate::tests::spawn_header_capture_server(CSRC_ERROR_HTML.to_string());

    let err = tauri::async_runtime::block_on(fetch_fund_quote_from(
        &client,
        &mut pacer,
        "002503",
        &[sina.as_str()],
        &[csrc.as_str()],
    ))
    .expect_err("披露源不可信应上抛");
    assert!(
        err.is_code("sync.disclosure-source-malformed"),
        "应报披露源不可信码化错误，实际 {err:?}"
    );
}

#[test]
fn batch_face_intercepted_fails_without_fallback() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (sina, _) =
        crate::tests::spawn_header_capture_server(b"<html>risk control</html>".to_vec());
    let (csrc, csrc_requests) =
        crate::tests::spawn_header_capture_server(CSRC_TERMINATED_PAYLOAD.to_string());

    let err = tauri::async_runtime::block_on(fetch_fund_quote_from(
        &client,
        &mut pacer,
        "002503",
        &[sina.as_str()],
        &[csrc.as_str()],
    ))
    .expect_err("批量面被拦截应上抛");
    assert!(
        err.is_code("sync.fund-batch-source-malformed"),
        "批量面取数失败应报批量源不可信码化错误，实际 {err:?}"
    );
    assert!(
        csrc_requests.lock().unwrap().is_empty(),
        "批量面取数失败不应触发官方披露请求"
    );
}
