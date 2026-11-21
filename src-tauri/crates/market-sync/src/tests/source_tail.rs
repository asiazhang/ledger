use std::time::Duration;

use ledger_infra::error::{AppError, Result};
use ledger_infra::test_utils::{CapturedEvent, capture_events};
use tracing::Level;

use crate::csrc::fetch_fund_nav_series_from;
use crate::ecb::fetch_ecb_90d_incremental;
use crate::http::{Pacer, RetryConfig};
use crate::sina_fund::{fetch_fund_nav_history, fetch_sina_fund_nav_rows};
use crate::tencent::fetch_tencent_batch;
use crate::tests::spawn_capture_server;

enum ExpectedError {
    Coded(&'static str),
    Parse,
}

impl ExpectedError {
    fn matches(&self, error: &AppError) -> bool {
        match self {
            ExpectedError::Coded(code) => error.is_code(code),
            ExpectedError::Parse => matches!(error, AppError::Parse(_)),
        }
    }

    fn describe(&self) -> String {
        match self {
            ExpectedError::Coded(code) => format!("码化错误 {code}"),
            ExpectedError::Parse => "AppError::Parse（内部错误不转专码，ADR-0050）".to_string(),
        }
    }
}

type Fetch = Box<dyn FnOnce(&reqwest::Client, &str, &mut Pacer) -> Result<()>>;

struct MalformedCase {
    label: &'static str,
    source: &'static str,
    status: u16,
    body: Vec<u8>,
    expected_head: String,
    expect: ExpectedError,
    fetch: Fetch,
}

fn head(text: &str) -> String {
    text.chars().take(120).collect()
}

fn assert_source_malformed_tail(case: MalformedCase) {
    let MalformedCase {
        label,
        source,
        status,
        body,
        expected_head,
        expect,
        fetch,
    } = case;
    let (url, _heads) = spawn_capture_server(move |_| (status, body.clone()));
    let client = reqwest::Client::new();
    let baseline = Duration::from_millis(10);
    let mut pacer = Pacer::new(baseline);

    let mut outcome: Option<AppError> = None;
    let events = capture_events(|| outcome = fetch(&client, &url, &mut pacer).err());

    let error = outcome.unwrap_or_else(|| panic!("[{label}] 畸形响应应报错"));
    assert!(
        expect.matches(&error),
        "[{label}] 期望{}，实际错误 {error:?}",
        expect.describe()
    );

    assert!(
        pacer.interval() > baseline,
        "[{label}] 源畸形必须补降速信号（基线 {baseline:?}），实际间隔 {:?}",
        pacer.interval()
    );

    let warns: Vec<&CapturedEvent> = events
        .iter()
        .filter(|event| event.level == Level::WARN)
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "[{label}] 源畸形处置恰留一条 warn（一次失败不两处各记一半），实际 {}: {warns:#?}",
        warns.len()
    );
    let event = warns[0];
    let message = field(event, "message").unwrap_or_else(|| panic!("[{label}] warn 应有文案"));
    assert!(
        message.contains("源畸形"),
        "[{label}] 固定文案应含「源畸形」（grep 锚），实际 {message:?}"
    );
    assert_eq!(
        field(event, "source"),
        Some(source),
        "[{label}] source 字段应是单元标识"
    );
    let error_field =
        field(event, "error").unwrap_or_else(|| panic!("[{label}] warn 应带 error 字段"));
    assert!(
        !error_field.is_empty(),
        "[{label}] error 字段应携带失败原因（解析器改回 Err(detail) 的意义所在）"
    );
    assert_eq!(
        field(event, "body"),
        Some(expected_head.as_str()),
        "[{label}] body 应是响应文本按字符 ×120 的截断片段"
    );
}

fn field<'a>(event: &'a CapturedEvent, key: &str) -> Option<&'a str> {
    event
        .fields
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

#[test]
fn tencent_batch_quote_tail_on_decode_failure() {
    let body = vec![0x81, 0x20];
    assert_source_malformed_tail(MalformedCase {
        label: "场内批量报价·GBK 解码失败",
        source: "tencent-batch-quote",
        status: 200,
        expected_head: head(&String::from_utf8_lossy(&body)),
        body,
        expect: ExpectedError::Coded("sync.quote-source-malformed"),
        fetch: Box::new(|client, url, pacer| {
            tauri::async_runtime::block_on(fetch_tencent_batch(client, pacer, &[url], "sh600000"))
                .map(|_| ())
        }),
    });
}

#[test]
fn sina_batch_nav_tail_keeps_decoded_text_head() {
    let text = format!("<html><body>{}\n</body></html>", "风险控制页面".repeat(40));
    let body = encoding_rs::GBK.encode(&text).0.into_owned();
    assert_source_malformed_tail(MalformedCase {
        label: "场外批量净值面·GBK 中文风控页",
        source: "sina-batch-nav",
        status: 200,
        expected_head: head(&text),
        body,
        expect: ExpectedError::Coded("sync.fund-batch-source-malformed"),
        fetch: Box::new(|client, url, pacer| {
            tauri::async_runtime::block_on(fetch_sina_fund_nav_rows(
                client,
                pacer,
                &[url],
                &["000001".to_string()],
            ))
            .map(|_| ())
        }),
    });
}

#[test]
fn sina_nav_history_tail_on_non_json_page() {
    let body = b"<html>blocked</html>".to_vec();
    assert_source_malformed_tail(MalformedCase {
        label: "单只全历史面·非 JSON 拦截页",
        source: "sina-nav-history",
        status: 200,
        expected_head: head(&String::from_utf8_lossy(&body)),
        body,
        expect: ExpectedError::Parse,
        fetch: Box::new(|client, url, pacer| {
            tauri::async_runtime::block_on(fetch_fund_nav_history(
                client,
                pacer,
                &[url],
                "000001",
                None,
                None,
            ))
            .map(|_| ())
        }),
    });
}

#[test]
fn csrc_disclosure_tail_on_server_error_page() {
    let body = "<html>系统异常</html>".as_bytes().to_vec();
    assert_source_malformed_tail(MalformedCase {
        label: "披露区间查询·500 系统异常页",
        source: "csrc-disclosure",
        status: 500,
        expected_head: head(&String::from_utf8_lossy(&body)),
        body,
        expect: ExpectedError::Coded("sync.disclosure-source-malformed"),
        fetch: Box::new(|client, url, pacer| {
            tauri::async_runtime::block_on(fetch_fund_nav_series_from(
                client,
                pacer,
                "110022",
                "2026-09-17",
                "2026-09-18",
                &[url],
            ))
            .map(|_| ())
        }),
    });
}

#[test]
fn ecb_document_tail_on_non_xml_page() {
    let body = b"<html>waf blocked</html>".to_vec();
    assert_source_malformed_tail(MalformedCase {
        label: "ECB 汇率文档·非 XML 拦截页",
        source: "ecb-document",
        status: 200,
        expected_head: head(&String::from_utf8_lossy(&body)),
        body,
        expect: ExpectedError::Coded("fx.source-malformed"),
        fetch: Box::new(|client, url, pacer| {
            tauri::async_runtime::block_on(fetch_ecb_90d_incremental(
                client,
                pacer,
                &[url],
                RetryConfig::production(),
            ))
            .map(|_| ())
        }),
    });
}
