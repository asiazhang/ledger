use std::time::Duration;

use crate::http::{
    KlineBar, Pacer, RetryConfig, request_json_from_hosts, request_json_with_retry,
    request_text_from_hosts,
};

use super::{spawn_capture_server, spawn_header_capture_server};

#[derive(Debug, serde::Deserialize)]
struct TestJson {
    data: Option<TestBody>,
}

#[derive(Debug, serde::Deserialize)]
struct TestBody {
    diff: Option<Vec<serde_json::Value>>,
}

fn fast_cfg(max_retries: u32, max_throttle_retries: u32) -> RetryConfig {
    RetryConfig {
        max_retries,
        base_backoff: Duration::from_millis(1),
        max_throttle_retries,
        throttle_cooldown: Duration::from_millis(1),
        request_timeout: Duration::from_secs(30),
    }
}

#[test]
fn pacer_slows_down_on_throttle_and_recovers_gradually_to_the_baseline() {
    let baseline = Duration::from_secs(1);
    let mut pacer = Pacer::new(baseline);
    assert_eq!(pacer.interval(), baseline, "正常状态贴近数据源可承受量级");

    pacer.record_throttled();
    assert_eq!(pacer.interval(), baseline * 2, "命限流即降速一档");
    pacer.record_throttled();
    assert_eq!(pacer.interval(), baseline * 4);
    for _ in 0..8 {
        pacer.record_throttled();
    }
    assert_eq!(pacer.interval(), Duration::from_secs(8), "降速有上限");

    let mut previous = pacer.interval();
    for _ in 0..40 {
        pacer.record_success();
        assert!(pacer.interval() <= previous, "回升单调不加速");
        assert!(pacer.interval() >= baseline, "回升不低过基线");
        previous = pacer.interval();
    }
    assert_eq!(pacer.interval(), baseline, "最终回到基线");
}

#[test]
fn pacer_zero_interval_stays_inert() {
    let mut pacer = Pacer::new(Duration::ZERO);
    pacer.record_throttled();
    pacer.record_success();
    assert_eq!(pacer.interval(), Duration::ZERO);
}

#[test]
fn throttle_responses_slow_the_request_interval() {
    let (url, _) = spawn_capture_server(|n| {
        if n == 1 {
            (200, "risk control page".into())
        } else {
            (200, r#"{"data":{"diff":[]}}"#.into())
        }
    });
    let client = reqwest::Client::new();
    let baseline = Duration::from_millis(20);
    let mut pacer = Pacer::new(baseline);
    let params = [("fs", "test")];
    let _ = tauri::async_runtime::block_on(request_json_with_retry::<TestJson>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(3, 3),
        None,
    ))
    .unwrap();
    assert!(
        pacer.interval() > baseline,
        "疑似风控页应把请求间隔降下来，实际 {:?}",
        pacer.interval()
    );

    let (url, _) = spawn_capture_server(|n| {
        if n == 1 {
            (429, "rate limited".into())
        } else {
            (200, r#"{"data":{"diff":[]}}"#.into())
        }
    });
    let mut pacer = Pacer::new(baseline);
    let _ = tauri::async_runtime::block_on(request_json_with_retry::<TestJson>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(3, 3),
        None,
    ))
    .unwrap();
    assert!(
        pacer.interval() > baseline,
        "限流响应应把请求间隔降下来，实际 {:?}",
        pacer.interval()
    );
}

#[test]
fn request_json_retries_429_then_succeeds() {
    let (url, _) = spawn_capture_server(|n| {
        if n == 1 {
            (429, "rate limited".into())
        } else {
            (200, r#"{"data":{"diff":[]}}"#.into())
        }
    });
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test"), ("pn", "1")];
    let json = tauri::async_runtime::block_on(request_json_with_retry::<TestJson>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(3, 3),
        None,
    ))
    .unwrap();
    assert_eq!(
        json.data.unwrap().diff.unwrap().len(),
        0,
        "解析成功即视为重试成功"
    );
}

#[test]
fn request_json_retries_on_json_decode_failure() {
    let (url, _) = spawn_capture_server(|n| {
        if n == 1 {
            (200, "not json at all".into())
        } else {
            (200, r#"{"data":{"diff":[]}}"#.into())
        }
    });
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let json = tauri::async_runtime::block_on(request_json_with_retry::<TestJson>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(3, 3),
        None,
    ))
    .unwrap();
    assert_eq!(
        json.data.unwrap().diff.unwrap().len(),
        0,
        "解析成功即视为重试成功"
    );
}

#[test]
fn request_json_returns_error_after_429_exhausted() {
    let (url, _) = spawn_capture_server(|_| (429, "rate limited".into()));
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let err = tauri::async_runtime::block_on(request_json_with_retry::<TestJson>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(2, 2),
        None,
    ))
    .unwrap_err();
    assert!(err.is_code("market.throttled"), "实际 {err:?}");
}

#[test]
fn request_json_returns_error_when_connection_refused() {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let url = "http://127.0.0.1:1/x".to_string();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let err = tauri::async_runtime::block_on(request_json_with_retry::<TestJson>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(2, 0),
        None,
    ))
    .unwrap_err();
    assert!(err.to_string().contains("HTTP 请求失败"));
}

#[test]
fn request_json_falls_back_to_next_host() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let hits = Arc::new(AtomicUsize::new(0));
    let h1 = hits.clone();
    let (url1, _) = spawn_capture_server(move |_| {
        h1.fetch_add(1, Ordering::SeqCst);
        (500, "boom".into())
    });
    let h2 = hits.clone();
    let (url2, _) = spawn_capture_server(move |_| {
        h2.fetch_add(1, Ordering::SeqCst);
        (200, r#"{"data":{"diff":[]}}"#.into())
    });

    let hosts = [url1.as_str(), url2.as_str()];
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let resp = tauri::async_runtime::block_on(request_json_from_hosts::<TestJson>(
        &client,
        &params,
        "/x",
        &hosts,
        fast_cfg(0, 0),
        &mut pacer,
        "test",
        None,
    ))
    .unwrap();
    assert_eq!(
        resp.data.unwrap().diff.unwrap().len(),
        0,
        "解析成功即视为主机切换成功"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[test]
fn request_json_returns_error_when_all_hosts_fail() {
    let (url, _) = spawn_capture_server(|_| (500, "boom".into()));
    let hosts = [url.as_str()];
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let err = tauri::async_runtime::block_on(request_json_from_hosts::<TestJson>(
        &client,
        &params,
        "/x",
        &hosts,
        fast_cfg(0, 0),
        &mut pacer,
        "test",
        None,
    ))
    .unwrap_err();
    assert!(err.is_code("market.request-exhausted"), "实际 {err:?}");
    assert!(err.to_string().contains("全部行情主机请求失败"));
}

fn spawn_truncated_body_server(hits: std::sync::Arc<std::sync::atomic::AtomicUsize>) -> String {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 1000\r\nConnection: close\r\n\r\nshort-but-ok",
            );
        }
    });
    url
}

#[test]
fn body_read_failure_retries_are_capped_then_converge_to_coded_error() {
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let url = spawn_truncated_body_server(hits.clone());
    let hosts = [url.as_str()];
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let err = tauri::async_runtime::block_on(request_text_from_hosts(
        &client,
        &[],
        "/x",
        &hosts,
        fast_cfg(0, 2),
        &mut pacer,
        "test",
        None,
    ))
    .unwrap_err();
    assert!(err.is_code("market.request-exhausted"), "实际 {err:?}");
    assert!(err.to_string().contains("读取响应失败"), "实际 {err:?}");
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        3,
        "读体失败应有界重试（1 首发 + max_throttle_retries 次冷却重试）"
    );
}

#[test]
fn ecb_full_history_request_timeout_is_sized_for_the_multi_mb_file() {
    assert_eq!(
        crate::ecb::full_history_cfg().request_timeout,
        Duration::from_secs(120),
    );
    assert_eq!(
        RetryConfig::production().request_timeout,
        Duration::from_secs(30),
    );
}

fn spawn_inflight_tracking_server(
    inflight: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    max_inflight: std::sync::Arc<std::sync::atomic::AtomicUsize>,
) -> String {
    use std::io::{Read, Write};
    use std::sync::atomic::Ordering;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let inflight = inflight.clone();
            let max_inflight = max_inflight.clone();
            std::thread::spawn(move || {
                let current = inflight.fetch_add(1, Ordering::SeqCst) + 1;
                max_inflight.fetch_max(current, Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                std::thread::sleep(Duration::from_millis(120));
                inflight.fetch_sub(1, Ordering::SeqCst);
                let body = r#"{"data":{"diff":[]}}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });
    url
}

#[test]
fn concurrent_requests_serialize_on_the_shared_pacer() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let inflight = Arc::new(AtomicUsize::new(0));
    let max_inflight = Arc::new(AtomicUsize::new(0));
    let url = spawn_inflight_tracking_server(inflight, max_inflight.clone());
    let pacer = Arc::new(tokio::sync::Mutex::new(Pacer::new(Duration::ZERO)));

    let mut handles = Vec::new();
    for _ in 0..2 {
        let pacer = pacer.clone();
        let url = url.clone();
        handles.push(std::thread::spawn(move || {
            tauri::async_runtime::block_on(async move {
                let client = reqwest::Client::new();
                let mut pacer = crate::http::lock_pacer(&pacer).await;
                crate::http::request_json_from_hosts::<TestJson>(
                    &client,
                    &[("fs", "test")],
                    "/x",
                    &[url.as_str()],
                    fast_cfg(0, 0),
                    &mut pacer,
                    "test",
                    None,
                )
                .await
                .unwrap();
            });
        }));
    }
    for handle in handles {
        handle.join().expect("请求线程不应 panic");
    }
    assert_eq!(
        max_inflight.load(Ordering::SeqCst),
        1,
        "相邻请求必须严格串行，服务端侧不得出现重叠在途（绕过共享 pacer 锁即红）"
    );
}

fn ecb_sample_xml(date: &str, cny: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<gesmes:Envelope xmlns:gesmes="http://www.gesmes.org/xml/2002-08-01" xmlns="http://www.ecb.int/vocabulary/2002-08-01/eurofxref"><Cube>
<Cube time="{date}"><Cube currency="USD" rate="1.146"/><Cube currency="HKD" rate="8.9903"/><Cube currency="CNY" rate="{cny}"/></Cube>
</Cube></gesmes:Envelope>"#
    )
}

#[test]
fn fetch_ecb_full_history_and_90d_entries_parse_fake_responses() {
    let (url, _) = spawn_capture_server({
        let body = ecb_sample_xml("2026-09-18", "7.6755");
        move |_| (200, body.clone().into())
    });
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let days = tauri::async_runtime::block_on(crate::ecb::fetch_ecb_full_history(
        &client,
        &mut pacer,
        &[url.as_str()],
        RetryConfig::production(),
    ))
    .unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].rates.get("HKD"), Some(&8.9903));
    let days = tauri::async_runtime::block_on(crate::ecb::fetch_ecb_90d_incremental(
        &client,
        &mut pacer,
        &[url.as_str()],
        RetryConfig::production(),
    ))
    .unwrap();
    assert_eq!(days[0].rates.get("CNY"), Some(&7.6755));
}

#[test]
fn fetch_ecb_entries_report_coded_error_on_unexpected_shapes() {
    let client = reqwest::Client::new();
    let (url, _) = spawn_capture_server(|_| (200, String::new().into()));
    let mut pacer = Pacer::new(Duration::ZERO);
    let err = tauri::async_runtime::block_on(crate::ecb::fetch_ecb_full_history(
        &client,
        &mut pacer,
        &[url.as_str()],
        RetryConfig::production(),
    ))
    .unwrap_err();
    assert!(err.is_code("fx.source-malformed"), "实际 {err:?}");
    let (url, _) = spawn_capture_server(|_| (200, "<html>waf blocked</html>".into()));
    let mut pacer = Pacer::new(Duration::ZERO);
    let err = tauri::async_runtime::block_on(crate::ecb::fetch_ecb_90d_incremental(
        &client,
        &mut pacer,
        &[url.as_str()],
        RetryConfig::production(),
    ))
    .unwrap_err();
    assert!(err.is_code("fx.source-malformed"), "实际 {err:?}");
}

#[test]
fn ecb_host_and_paths_pin_to_the_official_reference_rates_files() {
    assert_eq!(crate::ecb::ECB_HOSTS, ["https://www.ecb.europa.eu"]);
    assert_eq!(
        crate::ecb::FULL_HISTORY_PATH,
        "/stats/eurofxref/eurofxref-hist.xml"
    );
    assert_eq!(
        crate::ecb::INCREMENTAL_90D_PATH,
        "/stats/eurofxref/eurofxref-hist-90d.xml"
    );
}

const TENCENT_KLINE_BODY: &str = r#"{"code":0,"msg":"","data":{"sh600519":{"week":[["2026-09-11","1257.980","1266.980","1267.600","1254.000","17554.000"],["2026-09-18","1262.990","1257.120","1265.880","1256.100","24891.000"]],"qt":{"sh600519":["1","\u8d35\u5dde\u8305\u53f0","600519"]},"version":"16"}}}"#;

#[test]
fn fetch_tencent_week_kline_pins_request_shape() {
    let (url, heads) = spawn_header_capture_server(TENCENT_KLINE_BODY.to_string());
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let bars = tauri::async_runtime::block_on(crate::tencent_kline::fetch_tencent_week_kline(
        &client,
        &mut pacer,
        &[url.as_str()],
        "sh600519",
        "2024-09-19",
        "2026-09-19",
        crate::tencent_kline::KLINE_COUNT,
    ))
    .unwrap();
    assert_eq!(
        bars,
        vec![
            KlineBar::new("2026-09-11", 1266.98),
            KlineBar::new("2026-09-18", 1257.12),
        ]
    );
    let head = heads.lock().unwrap().first().cloned().unwrap_or_default();
    assert!(
        head.contains(
            "GET /appstock/app/fqkline/get?param=sh600519%2Cweek%2C2024-09-19%2C2026-09-19%2C1200%2C HTTP/1.1"
        ),
        "请求形态须为「查询键,week,起始,结束,根数,」（末段空 = 不复权），实际请求头：{head}"
    );
}

#[test]
fn fetch_tencent_week_kline_accepts_shorter_series_than_requested() {
    let body = r#"{"code":0,"msg":"","data":{"sh600519":{"week":[["2026-09-18","1262.990","1257.120","1265.880","1256.100","24891.000"]],"version":"16"}}}"#;
    let (url, heads) = spawn_header_capture_server(body.to_string());
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let bars = tauri::async_runtime::block_on(crate::tencent_kline::fetch_tencent_week_kline(
        &client,
        &mut pacer,
        &[url.as_str()],
        "sh600519",
        "2024-09-19",
        "2026-09-19",
        crate::tencent_kline::KLINE_COUNT,
    ))
    .unwrap();
    assert_eq!(bars, vec![KlineBar::new("2026-09-18", 1257.12)]);
    let head = heads.lock().unwrap().first().cloned().unwrap_or_default();
    assert!(
        head.contains("%2C1200%2C"),
        "短返回是服务端行为：请求仍须带上要的根数，实际请求头：{head}"
    );
}

#[test]
fn fetch_tencent_week_kline_returns_empty_for_invalid_code() {
    let body = r#"{"code":0,"msg":"","data":{"sh999999":{"week":[],"version":"16"}}}"#;
    let (url, _) = spawn_header_capture_server(body.to_string());
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let bars = tauri::async_runtime::block_on(crate::tencent_kline::fetch_tencent_week_kline(
        &client,
        &mut pacer,
        &[url.as_str()],
        "sh999999",
        "2024-09-19",
        "2026-09-19",
        crate::tencent_kline::KLINE_COUNT,
    ))
    .unwrap();
    assert!(bars.is_empty(), "无效代码应返回空序列而非错误");
}

#[test]
fn tencent_kline_host_and_path_pin_to_the_quote_site() {
    assert_eq!(
        crate::tencent_kline::TENCENT_KLINE_HOSTS,
        ["https://web.ifzq.gtimg.cn"]
    );
    assert_eq!(
        crate::tencent_kline::TENCENT_KLINE_PATH,
        "/appstock/app/fqkline/get"
    );
}
