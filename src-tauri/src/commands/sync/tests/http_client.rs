//! HTTP 客户端层（issue #89）：请求重试、多主机切换与错误传播。
//! 经本地 HTTP 服务独立测试，不依赖真实网络。

use std::time::Duration;

use crate::commands::sync::http::{
    ClistResponse, Pacer, RetryConfig, request_json_from_hosts, request_json_with_retry,
};

fn fast_cfg(max_retries: u32, max_throttle_retries: u32) -> RetryConfig {
    RetryConfig {
        max_retries,
        base_backoff: Duration::from_millis(1),
        max_throttle_retries,
        throttle_cooldown: Duration::from_millis(1),
    }
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
            (200, r#"{"data":{"total":7}}"#.into())
        }
    });
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test"), ("pn", "1")];
    let json = request_json_with_retry::<ClistResponse>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(3, 3),
    )
    .unwrap();
    assert_eq!(json.data.total, Some(7));
}

#[test]
fn request_json_retries_on_json_decode_failure() {
    let url = spawn_http_server(|n| {
        if n == 1 {
            (200, "not json at all".into())
        } else {
            (200, r#"{"data":{"total":9}}"#.into())
        }
    });
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let json = request_json_with_retry::<ClistResponse>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(3, 3),
    )
    .unwrap();
    assert_eq!(json.data.total, Some(9));
}

#[test]
fn request_json_returns_error_after_429_exhausted() {
    let url = spawn_http_server(|_| (429, "rate limited".into()));
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let err = request_json_with_retry::<ClistResponse>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(2, 2),
    )
    .unwrap_err();
    assert!(err.to_string().contains("429"));
}

#[test]
fn request_json_returns_error_when_connection_refused() {
    // 显式禁用系统代理：默认 Client 会读取系统代理（如 Clash/Surge 监听 127.0.0.1），
    // 代理转发到无监听的端口时会返回空 body 响应，导致“连接被拒绝”语义失效。
    // 目标用保留端口 1，本机几乎不可能有服务监听，可稳定触发 ECONNREFUSED。
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let url = "http://127.0.0.1:1/x".to_string();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let err = request_json_with_retry::<ClistResponse>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(2, 0),
    )
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
        (200, r#"{"data":{"total":7}}"#.into())
    });

    let hosts = [url1.as_str(), url2.as_str()];
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let resp = request_json_from_hosts::<ClistResponse>(
        &client,
        &params,
        "/x",
        &hosts,
        fast_cfg(0, 0),
        &mut pacer,
        "test",
    )
    .unwrap();
    assert_eq!(resp.data.total, Some(7));
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[test]
fn request_json_returns_error_when_all_hosts_fail() {
    let url = spawn_http_server(|_| (500, "boom".into()));
    let hosts = [url.as_str()];
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let err = request_json_from_hosts::<ClistResponse>(
        &client,
        &params,
        "/x",
        &hosts,
        fast_cfg(0, 0),
        &mut pacer,
        "test",
    )
    .unwrap_err();
    assert!(err.to_string().contains("全部行情主机请求失败"));
}
