//! HTTP 客户端层（issue #89）：请求重试、多主机切换与错误传播。
//! 经本地 HTTP 服务独立测试，不依赖真实网络。

use std::time::Duration;

use crate::http::{KlineBar, Pacer, RetryConfig, request_json_from_hosts, request_json_with_retry};

use super::spawn_header_capture_server;

/// HTTP 层测试的通用应答形状（ulist 通道退役后改为最小本地形状，测试只关心
/// 「能否解析出 JSON 对象」）。
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
    }
}

// ---------------------------------------------------------------------------
// 自适应限速（ADR-0121 决策 5 / issue #1374）：不再写死每请求 2 秒——正常停在
// 「数据源可承受量级」的基线，遇限流响应 / 疑似风控页降速并复用既有冷却，恢复后
// 逐步回升到基线。
// ---------------------------------------------------------------------------

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

    // 恢复：每次成功只回升一步（不一步跳回基线——风控窗口刚过就重新撞上是坏性格）。
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
    // 测试把间隔传零时，自适应逻辑不得凭空产生等待（既有用例的构造面不变）。
    let mut pacer = Pacer::new(Duration::ZERO);
    pacer.record_throttled();
    pacer.record_success();
    assert_eq!(pacer.interval(), Duration::ZERO);
}

#[test]
fn throttle_responses_slow_the_request_interval() {
    // 疑似风控页（200 + 非 JSON）与 429 都是「对方在限我们」的信号：降速一档，
    // 冷却等待复用既有 throttle_cooldown（本用例把它压到 1ms）。
    let url = spawn_http_server(|n| {
        if n == 1 {
            (200, "risk control page".into())
        } else {
            (200, r#"{"data":{"diff":[]}}"#.into())
        }
    });
    let client = reqwest::Client::new();
    // 基线只作「相对降速」的锚（断言都是 `interval() > baseline` 这类比较），
    // 绝对值不承载语义——压到 20ms 让 pacer 的整数倍等待不再贡献秒级墙钟
    // （spec #1086 / issue #1514 同款手法）。
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

    let url = spawn_http_server(|n| {
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

/// 起一个本地 HTTP 服务，按调用次数回调响应 (status, body)，返回基础地址。
fn spawn_http_server(responder: impl Fn(usize) -> (u16, String) + Send + 'static) -> String {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let mut seq = 0usize;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            seq += 1;
            let (status, body) = responder(seq);
            let reason = if status == 200 { "OK" } else { "Limited" };
            let resp = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    url
}

#[test]
fn request_json_retries_429_then_succeeds() {
    let url = spawn_http_server(|n| {
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
    let url = spawn_http_server(|n| {
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
    let url = spawn_http_server(|_| (429, "rate limited".into()));
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
    assert!(err.to_string().contains("429"));
}

#[test]
fn request_json_returns_error_when_connection_refused() {
    // 显式禁用系统代理：默认 Client 会读取系统代理（如 Clash/Surge 监听 127.0.0.1），
    // 代理转发到无监听的端口时会返回空 body 响应，导致“连接被拒绝”语义失效。
    // 目标用保留端口 1，本机几乎不可能有服务监听，可稳定触发 ECONNREFUSED。
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
    let url1 = spawn_http_server(move |_| {
        h1.fetch_add(1, Ordering::SeqCst);
        (500, "boom".into())
    });
    let h2 = hits.clone();
    let url2 = spawn_http_server(move |_| {
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
    let url = spawn_http_server(|_| (500, "boom".into()));
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
    assert!(err.to_string().contains("全部行情主机请求失败"));
}

// ---------------------------------------------------------------------------
// 强串行（ADR-0125 决策 6 / ADR-0087 负向判据）：异步化只换等待原语，相邻请求
// 仍严格串行——共享 pacer 的异步互斥体从发请求前一直持有到响应处理完。绕过串行
// （不取共享 pacer 锁即发请求）本用例即红。
// ---------------------------------------------------------------------------

/// 起一个多线程本地服务：每连接独立线程，记录服务端同时刻在途请求数与峰值。
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
                // 拉长处理窗口，让「绕过串行」的并发在途能被观测到；先减计数再
                // 回响应，保证串行下前一连接的计数已在下一连接到达前归零。
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

/// 两个线程经同一把共享 pacer 锁并发发起请求：服务端侧峰值在途必须恒为 1。
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
                // 共享 pacer 锁从发请求前持有到响应处理完——与生产通道束同形。
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

// ---------------------------------------------------------------------------
// ECB 参考汇率取数入口（issue #1542 / ADR-0019 修订记录）：两个入口都经本地
// HTTP 服务以假响应驱动，不依赖真实网络；非预期形状报 fx.source-malformed
// 码化错误，不静默产出空序列。
// ---------------------------------------------------------------------------

/// 最小真实形状的 ECB Cube 报文（gesmes 前缀 + 默认命名空间 + 自闭腿条目）。
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
    let url = spawn_http_server({
        let body = ecb_sample_xml("2026-09-18", "7.6755");
        move |_| (200, body.clone())
    });
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    // 全量历史入口：假响应驱动出日快照序列（解析正确性钉值）。
    let days = tauri::async_runtime::block_on(crate::ecb::fetch_ecb_full_history(
        &client,
        &mut pacer,
        &[url.as_str()],
    ))
    .unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].rates.get("HKD"), Some(&8.9903));
    // 90 天增量入口：同一报文形状、不同文件路径，同样可解析。
    let days = tauri::async_runtime::block_on(crate::ecb::fetch_ecb_90d_incremental(
        &client,
        &mut pacer,
        &[url.as_str()],
    ))
    .unwrap();
    assert_eq!(days[0].rates.get("CNY"), Some(&7.6755));
}

#[test]
fn fetch_ecb_entries_report_coded_error_on_unexpected_shapes() {
    let client = reqwest::Client::new();
    // 空文件（200 + 空 body）：报 fx.source-malformed，不产出空序列。
    let url = spawn_http_server(|_| (200, String::new()));
    let mut pacer = Pacer::new(Duration::ZERO);
    let err = tauri::async_runtime::block_on(crate::ecb::fetch_ecb_full_history(
        &client,
        &mut pacer,
        &[url.as_str()],
    ))
    .unwrap_err();
    assert!(err.is_code("fx.source-malformed"), "实际 {err:?}");
    // 非 XML（被拦截页）：同码报错。
    let url = spawn_http_server(|_| (200, "<html>waf blocked</html>".into()));
    let mut pacer = Pacer::new(Duration::ZERO);
    let err = tauri::async_runtime::block_on(crate::ecb::fetch_ecb_90d_incremental(
        &client,
        &mut pacer,
        &[url.as_str()],
    ))
    .unwrap_err();
    assert!(err.is_code("fx.source-malformed"), "实际 {err:?}");
}

/// 生产主机与两条文件路径的接线钉（删除即红）：入口默认指向 ECB 官方站的
/// 全量 / 90 天增量文件，换源或改路径须显式改此处。
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

// ---------------------------------------------------------------------------
// 腾讯日线 K 线取数入口（issue #1559 / ADR-0130 决策 2）：请求形态、服务端压缩
// 与无效代码处置都经本地 HTTP 服务钉住，不依赖真实网络。
// ---------------------------------------------------------------------------

/// 最小真实形状的腾讯日线响应（2026-09-19 实测形状：`day` 行与信封键原样，
/// `qt` 旁路字段 trim）。
const TENCENT_KLINE_BODY: &str = r#"{"code":0,"msg":"","data":{"sh600519":{"day":[["2026-09-17","1257.980","1266.980","1267.600","1254.000","17554.000"],["2026-09-18","1262.990","1257.120","1265.880","1256.100","24891.000"]],"qt":{"sh600519":["1","\u8d35\u5dde\u8305\u53f0","600519"]},"version":"16"}}}"#;

/// 请求形态钉（issue #1559 AC：本地 HTTP 服务用例钉住请求形态）：路径 / 查询键 /
/// 周期 / 区间 / 根数 / 末段空（不复权）逐段比对，改任一段即红。
#[test]
fn fetch_tencent_day_kline_pins_request_shape() {
    let (url, heads) = spawn_header_capture_server(TENCENT_KLINE_BODY.to_string());
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let bars = tauri::async_runtime::block_on(crate::tencent_kline::fetch_tencent_day_kline(
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
            KlineBar::new("2026-09-17", 1266.98),
            KlineBar::new("2026-09-18", 1257.12),
        ]
    );
    let head = heads.lock().unwrap().first().cloned().unwrap_or_default();
    assert!(
        head.contains(
            "GET /appstock/app/fqkline/get?param=sh600519%2Cday%2C2024-09-19%2C2026-09-19%2C800%2C HTTP/1.1"
        ),
        "请求形态须为「查询键,day,起始,结束,根数,」（末段空 = 不复权），实际请求头：{head}"
    );
}

/// 根数与区间上限行为（issue #1559 AC）：请求区间两年、根数 800，而服务端只回 1
/// 根（区间裁剪 / 压缩；研究文档 §4.2 记复权形态 1000 / 2000 曾被压回 640）时不
/// 报错、不补偿，按返回照常解析；请求确实要了 800 根，短返回是服务端行为而不是
/// 我们少要。
#[test]
fn fetch_tencent_day_kline_accepts_shorter_series_than_requested() {
    let body = r#"{"code":0,"msg":"","data":{"sh600519":{"day":[["2026-09-18","1262.990","1257.120","1265.880","1256.100","24891.000"]],"version":"16"}}}"#;
    let (url, heads) = spawn_header_capture_server(body.to_string());
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let bars = tauri::async_runtime::block_on(crate::tencent_kline::fetch_tencent_day_kline(
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
        head.contains("%2C800%2C"),
        "短返回是服务端行为：请求仍须带上要的根数，实际请求头：{head}"
    );
}

/// 无效代码返回空序列而非错误（issue #1559 AC：补全不被中断）：`code` 仍为 0、
/// `day` 为空数组，取数入口直接回空序列而不是 Err。
#[test]
fn fetch_tencent_day_kline_returns_empty_for_invalid_code() {
    let body = r#"{"code":0,"msg":"","data":{"sh999999":{"day":[],"version":"16"}}}"#;
    let (url, _) = spawn_header_capture_server(body.to_string());
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let bars = tauri::async_runtime::block_on(crate::tencent_kline::fetch_tencent_day_kline(
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

/// 生产主机与接口路径的接线钉（删除即红）：入口默认指向腾讯 K 线站与
/// `fqkline/get` 路径，换源或改路径须显式改此处。
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
