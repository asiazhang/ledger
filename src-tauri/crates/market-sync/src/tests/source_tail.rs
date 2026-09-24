//! 取数尾部契约的跨单元畸形语料（spec #1675）：对**每个取数单元**注入一份畸形
//! 响应（本地 HTTP 服务，#1582 上收的既有 seam），共享同一断言函数钉住三件套——
//!
//! 1. **降速被记录**：pacer 间隔高于基线（读限速器的可观察面，不断言契约函数被
//!    调用；各用例自持独立 pacer，不经进程级单例）；
//! 2. **单条源畸形 warn**：固定「源畸形」文案 + `source`（单元标识）/ `error`
//!    （错误详情）/ `body`（响应文本按字符 ×120 的截断片段）——恰一条，解析器内部
//!    的形状 warn 已上收、一次失败不再两处各记一半；GBK 通道的 `body` 钉**解码后
//!    文本**（不是字节有损片段——那会把中文报文截成乱码），解码失败时才有损兜底；
//! 3. **期望的码化错误**：各单元码值不变（分码是既定契约），场内批量报价值
//!    `sync.quote-source-malformed`（spec #1675 裁决 3 新增专码）。
//!
//! 删除即变红：从任一取数单元删掉契约接线（改回直调解析、自行补降速或散落 warn），
//! 该单元的用例三件套至少一条断言变红，断言全部对准可观察的降速记录、日志字段与
//! 码化错误，不对准契约函数的调用形状（ADR-0087 断言强度）。

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

/// 期望错误形态：专码族（用户可见的源畸形一律专码，ADR-0050）或保留的内部
/// Parse（新浪单只全历史面——消费面在后台补全、不直达用户，spec #1675 裁决 3）。
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

/// 单元取数入口的驱动闭包：（客户端, 本地服务地址, 独立 pacer）→ 丢弃成功载荷。
type Fetch = Box<dyn FnOnce(&reqwest::Client, &str, &mut Pacer) -> Result<()>>;

/// 一份跨单元语料：单元标识 + 畸形响应 + 期望错误 + 该单元取数入口的驱动闭包。
struct MalformedCase {
    /// 断言失败信息的用例名。
    label: &'static str,
    /// 期望的 `source` 日志字段（单元标识；字面量钉住，漂移即红）。
    source: &'static str,
    /// 本地 HTTP 服务应答状态码。
    status: u16,
    /// 畸形响应体（含 GBK 中文页与非法字节两种形态）。
    body: Vec<u8>,
    /// 期望的 `body` 截断片段（按字符 ×120 的独立复算，锚定「取解码后文本」）。
    expected_head: String,
    /// 期望错误形态。
    expect: ExpectedError,
    /// 单元取数入口驱动。
    fetch: Fetch,
}

/// 文本（或有损转换结果）的截断片段期望值：按字符 ×120。
fn head(text: &str) -> String {
    text.chars().take(120).collect()
}

/// 共享断言函数：语料的每一行都经此钉住三件套（见模块文档）。
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

    // ① 期望的码化错误（fail-closed，不退化为「无数据」）。
    let error = outcome.unwrap_or_else(|| panic!("[{label}] 畸形响应应报错"));
    assert!(
        expect.matches(&error),
        "[{label}] 期望{}，实际错误 {error:?}",
        expect.describe()
    );

    // ② 降速被记录：pacer 间隔高于基线（解析失败必降速，ADR-0121 决策 5）。
    assert!(
        pacer.interval() > baseline,
        "[{label}] 源畸形必须补降速信号（基线 {baseline:?}），实际间隔 {:?}",
        pacer.interval()
    );

    // ③ 单条源畸形 warn，固定文案 + source / error / body 三字段。
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

/// 捕获事件的字段取值。
fn field<'a>(event: &'a CapturedEvent, key: &str) -> Option<&'a str> {
    event
        .fields
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

// ---------------------------------------------------------------------------
// 跨单元语料：五个取数单元各一行（共享上面的断言函数）
// ---------------------------------------------------------------------------

/// 场内批量报价（腾讯，GBK 通道）：非法 GBK 字节——解码失败与形状判据失败走
/// 同一条源畸形路径（此路无解码文本，`body` 有损兜底），期望新专码
/// `sync.quote-source-malformed`（裁决 3：用户可见的源畸形一律专码，解析 detail
/// 退出用户面、留日志）。
#[test]
fn tencent_batch_quote_tail_on_decode_failure() {
    // GBK 非法序列：有效领字节 0x81 + 非法尾字节 0x20（尾字节域 0x40–0xFE 之外），
    // 解码必报错——解码失败无可信文本可截，`body` 走有损兜底、与形状失败同途。
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

/// 场外基金批量净值面（新浪，GBK 通道）：超长**中文**风控页——GBK 解码成功、形状
/// 判据失败：`body` 必须是解码后文本的截断片段（字节有损转换会截成乱码，此断言
/// 即该回归的守门），顺带钉住按字符 ×120。
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

/// 场外基金单只全历史面（新浪，文本通道）：非 JSON 拦截页——消费面在后台补全、
/// 不直达用户，按裁决 3 留 `AppError::Parse`。
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

/// 披露区间查询（证监会，文本通道）：500「系统异常」页——打 spec 点名的区间
/// 查询完整入口（翻页 + 完整性核验的取数单元入口，第一页即畸形），码值不变。
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

/// ECB 汇率文档（文本通道）：非 XML 拦截页——本单元原是五单元中唯一**漏补降速
/// 信号**的（ADR-0121 决策 5 漏执行，spec #1675 裁决 4 缺陷修复）；契约接线后
/// 与其他单元同形：截断日志 + 降速 + `fx.source-malformed` 码值不变。
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
