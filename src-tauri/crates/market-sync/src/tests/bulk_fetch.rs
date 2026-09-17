//! 行情批量取数面（ADR-0121 / issue #1374）：两条面报文的解析与可信度判据、
//! 请求形态（路径 / 参数 / Referer）、被拦截响应处置与跨同步记忆状态机。
//! 报文用真实形状的 fixture 驱动，请求形态经本地 HTTP 服务验证，不依赖真实网络。

use std::time::{Duration, Instant};

use crate::bulk::{
    BULK_DISABLE_PERIOD, BULK_FAILURE_THRESHOLD, BulkFetchCircuit, fetch_fund_name_dictionary_from,
    fetch_fund_nav_table_from, parse_fund_name_dictionary, parse_fund_nav_table,
};
use crate::http::Pacer;
use crate::tests::spawn_header_capture_server;

/// 名称全量字典真实报文形状（2026-09-15 实测 `fundcode_search.js` 截取）：每行
/// `[代码, 拼音缩写, 名称, 类型, 全拼]`；货币基金（000198）在其列——排行批量面
/// 不收录货基，名称字典收录，两者覆盖率不同正是「缺口按条回退」的来源。
const NAME_DICTIONARY_PAYLOAD: &str = r#"var r = [["000001","HXCZHH","华夏成长混合","混合型-灵活","HUAXIACHENGZHANGHUNHE"],["000198","THYEBHB","天弘余额宝货币","货币型-普通货币","TIANHONGYUEBAOHUOBI"],["110022","YFDXFHYGP","易方达消费行业股票","股票型","YIFANGDAXIAOFEIHANGYEGUPIAO"]];"#;

/// 场外基金净值排行真实报文形状（2026-09-15 实测 `rankhandler.aspx` 截取，列已
/// 截断到前六列 + 尾部标记）：每行 `代码,简称,拼音,净值日期,单位净值,累计净值,…`。
const NAV_TABLE_PAYLOAD: &str = r#"var rankData = {datas:["002910,易方达供给改革混合,YFDGJGGHH,2026-09-15,8.0624,8.0624,-0.55","110022,易方达消费行业股票,YFDXFHYGP,2026-09-15,3.3480,3.3480,0.12","021179,某新基金A,MXJJJ,,,",""],allRecords:20360,pageIndex:1};"#;

#[test]
fn name_dictionary_parses_code_and_authoritative_name() {
    let dictionary = parse_fund_name_dictionary(NAME_DICTIONARY_PAYLOAD).expect("真实报文应可解析");
    assert_eq!(
        dictionary.get("000001").map(String::as_str),
        Some("华夏成长混合")
    );
    assert_eq!(
        dictionary.get("110022").map(String::as_str),
        Some("易方达消费行业股票")
    );
    assert_eq!(
        dictionary.get("000198").map(String::as_str),
        Some("天弘余额宝货币"),
        "名称字典覆盖货币基金（排行批量面不收录它们）"
    );
}

#[test]
fn name_dictionary_rejects_blocked_page() {
    // 风控拦截页（HTML）或变量改名：数据数组不可信 → None，调用方回退逐只名称通道。
    assert_eq!(
        parse_fund_name_dictionary("<html><body>403</body></html>"),
        None
    );
    assert_eq!(parse_fund_name_dictionary("var x = [];"), None);
}

#[test]
fn nav_table_parses_code_date_and_unit_nav() {
    let table = parse_fund_nav_table(NAV_TABLE_PAYLOAD).expect("真实报文应可解析");
    let point = table.get("110022").expect("排行面收录该基金");
    assert_eq!(point.date, "2026-09-15");
    assert_eq!(point.nav, 3.348);
    // 未公布净值的新基金（日期与净值都缺）与空行：按未覆盖处理（缺口 → 逐只通道），
    // 不伪造一个「净值 0」的条目。
    assert!(!table.contains_key("021179"));
    assert_eq!(table.len(), 2);
}

#[test]
fn nav_table_rejects_no_permission_payload() {
    // 缺 Referer 时接口返回 `var rankData ={ErrCode:-999,Data:"无访问权限"}`（实测）：
    // 没有 datas 数组 → 不可信 → None，调用方 fail-closed 回退逐只净值通道。
    assert_eq!(
        parse_fund_nav_table(r#"var rankData ={ErrCode:-999,Data:"无访问权限"}"#),
        None
    );
    assert_eq!(parse_fund_nav_table("<html>风控</html>"), None);
}

#[test]
fn name_dictionary_fetch_reads_the_static_data_file() {
    let (url, heads) = spawn_header_capture_server(NAME_DICTIONARY_PAYLOAD.to_string());
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);

    let dictionary = tauri::async_runtime::block_on(fetch_fund_name_dictionary_from(
        &client,
        &mut pacer,
        &[url.as_str()],
    ))
    .expect("报文正常时应命中");

    let head = &heads.lock().unwrap()[0];
    assert!(
        head.contains("GET /js/fundcode_search.js"),
        "名称全量字典是静态数据文件: {head}"
    );
    assert_eq!(
        dictionary.get("110022").map(String::as_str),
        Some("易方达消费行业股票")
    );
}

#[test]
fn nav_table_fetch_sends_referer_and_pulls_the_whole_market_page() {
    let (url, heads) = spawn_header_capture_server(NAV_TABLE_PAYLOAD.to_string());
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);

    let table = tauri::async_runtime::block_on(fetch_fund_nav_table_from(
        &client,
        &mut pacer,
        &[url.as_str()],
    ))
    .expect("报文正常时应命中");

    let head = &heads.lock().unwrap()[0];
    assert!(head.contains("GET /data/rankhandler.aspx?"), "{head}");
    assert!(head.contains("op=ph"), "{head}");
    assert!(head.contains("dt=kf"), "{head}");
    assert!(head.contains("ft=all"), "{head}");
    assert!(
        head.contains("pn=30000"),
        "单页拉满即一次请求覆盖全市场（实测 20,360 只）: {head}"
    );
    assert!(
        head.to_lowercase()
            .contains("referer: https://fund.eastmoney.com/data/fundranking.html"),
        "缺 Referer 会被接口以「无访问权限」拦截: {head}"
    );
    assert_eq!(table.len(), 2);
}

#[test]
fn blocked_pages_fail_closed_instead_of_reporting_no_coverage() {
    // 被拦截（200 + 风控 HTML）与「确实零覆盖」必须分开：前者返回 Err 由调用方
    // fail-closed 回退逐只通道；后者才是缺口。把两者混同会把风控静默当作「这些
    // 标的本来就没有净值」。文本通道的解析恒成功，风控页在 HTTP 层看不见——
    // 降速信号由这一层补上（ADR-0121 决策 5），否则最容易被拦的面反而提速。
    let (url, _heads) = spawn_header_capture_server("<html>risk control</html>".to_string());
    let client = reqwest::Client::new();
    let baseline = Duration::from_secs(1);
    let mut pacer = Pacer::new(baseline);

    assert!(
        tauri::async_runtime::block_on(fetch_fund_name_dictionary_from(
            &client,
            &mut pacer,
            &[url.as_str()]
        ))
        .is_err(),
        "被拦截的名称字典不得当作可信结果"
    );
    assert!(
        pacer.interval() > baseline,
        "疑似风控页应把请求间隔降下来，实际 {:?}",
        pacer.interval()
    );
    let after_blocks = pacer.interval();
    assert!(
        tauri::async_runtime::block_on(fetch_fund_nav_table_from(
            &client,
            &mut pacer,
            &[url.as_str()]
        ))
        .is_err(),
        "被拦截的净值批量面不得当作可信结果"
    );
    assert!(
        pacer.interval() > after_blocks,
        "每次疑似风控响应都要继续降速，实际 {:?}",
        pacer.interval()
    );
}

// ---------------------------------------------------------------------------
// 跨同步记忆（ADR-0121 决策 3）：连续失败达阈值停用一个期限，到期先半开试一次
// ---------------------------------------------------------------------------

#[test]
fn circuit_opens_only_after_threshold_consecutive_failures() {
    let mut circuit = BulkFetchCircuit::new();
    let now = Instant::now();

    for _ in 1..BULK_FAILURE_THRESHOLD {
        circuit.record_failure(now);
        assert!(
            circuit.should_attempt(now),
            "未达阈值不停用（避免把偶发一次失败升级成长期停用）"
        );
    }
    circuit.record_failure(now);
    assert!(circuit.is_disabled(now), "达阈值即停用");
    assert!(!circuit.should_attempt(now), "停用期内不再撞批量面");
    assert!(!circuit.should_attempt(now + BULK_DISABLE_PERIOD / 2));
}

#[test]
fn circuit_success_resets_the_consecutive_failure_count() {
    let mut circuit = BulkFetchCircuit::new();
    let now = Instant::now();

    for _ in 1..BULK_FAILURE_THRESHOLD {
        circuit.record_failure(now);
    }
    circuit.record_success();
    assert_eq!(circuit.consecutive_failures(), 0);
    circuit.record_failure(now);
    assert!(
        circuit.should_attempt(now),
        "成功清零后重新计数，未达阈值不停用"
    );
}

#[test]
fn circuit_half_opens_once_after_the_disable_period_then_recovers_on_success() {
    let mut circuit = BulkFetchCircuit::new();
    let now = Instant::now();
    for _ in 0..BULK_FAILURE_THRESHOLD {
        circuit.record_failure(now);
    }

    let after_period = now + BULK_DISABLE_PERIOD;
    assert!(circuit.should_attempt(after_period), "停用到期先半开试一次");
    assert!(
        !circuit.should_attempt(after_period),
        "半开试探只放行一次，结果未回来之前不再放行"
    );
    assert!(
        !circuit.should_attempt(after_period + BULK_DISABLE_PERIOD / 2),
        "试探窗口同样是整个停用期限"
    );

    circuit.record_success();
    assert!(
        circuit.should_attempt(after_period),
        "试探成功即解除停用、恢复常态"
    );
}

#[test]
fn circuit_failed_half_open_trial_restarts_the_disable_period() {
    let mut circuit = BulkFetchCircuit::new();
    let now = Instant::now();
    for _ in 0..BULK_FAILURE_THRESHOLD {
        circuit.record_failure(now);
    }

    let after_period = now + BULK_DISABLE_PERIOD;
    assert!(circuit.should_attempt(after_period));
    circuit.record_failure(after_period);
    assert!(
        !circuit.should_attempt(after_period),
        "半开试探失败即重新停用一个期限"
    );
    assert!(circuit.should_attempt(after_period + BULK_DISABLE_PERIOD));
}
