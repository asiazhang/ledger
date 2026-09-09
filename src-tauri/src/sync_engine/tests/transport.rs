//! Transport 哑字节通道（issue #859 / ADR-0091 决策 1）：WebDAV 后端对本地
//! WebDAV 桩的读写行为、错误归一（凭据错误/网络失败 → 明确可重试的码化错误）
//! 与路径守卫。HTTP 语义全部对准本地桩，不依赖真实网络。

use crate::sync_engine::tests::common::spawn_webdav_stub;
use crate::sync_engine::transport::Transport;
use crate::sync_engine::transport::webdav::{WebDavConfig, WebDavTransport};

/// 可用的 WebDAV 通道（对桩、无认证）。
fn transport(base_url: &str) -> WebDavTransport {
    WebDavTransport::new(WebDavConfig {
        base_url: base_url.to_string(),
        username: "user".into(),
        password: "pass".into(),
    })
    .unwrap()
}

/// PUT 写入 → GET 读回逐字节一致；不存在的文件返回 None（增量拉取的常态输入）。
#[test]
fn webdav_put_get_roundtrip_and_missing_is_none() {
    let stub = spawn_webdav_stub(None);
    let dav = transport(&stub.base_url);

    let payload: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
    dav.write_file("book-x/streams/d1/seg-0000000001-0000000002.enc", &payload)
        .unwrap();
    let got = dav
        .read_file("book-x/streams/d1/seg-0000000001-0000000002.enc")
        .unwrap();
    assert_eq!(got.as_deref(), Some(payload.as_slice()));
    assert!(
        dav.read_file("book-x/streams/d1/absent.enc")
            .unwrap()
            .is_none()
    );
}

/// ensure_dir 逐级建目录且幂等（已存在不算错误）；目录就绪后写入成功。
#[test]
fn webdav_ensure_dir_is_idempotent_and_enables_nested_writes() {
    let stub = spawn_webdav_stub(None);
    let dav = transport(&stub.base_url);

    dav.ensure_dir("book-x/streams/d1").unwrap();
    dav.ensure_dir("book-x/streams/d1").unwrap();
    dav.write_file("book-x/streams/d1/manifest-probe.enc", b"bytes")
        .unwrap();
    assert_eq!(
        dav.read_file("book-x/streams/d1/manifest-probe.enc")
            .unwrap(),
        Some(b"bytes".to_vec())
    );
}

/// 凭据错误：读、写、建目录全部报 `sync-channel.auth-failed`（明确、可重试
/// ——修正凭据后重试即可），不裸上抛 HTTP 细节。
#[test]
fn webdav_wrong_credentials_report_retryable_auth_error() {
    let stub = spawn_webdav_stub(Some(("alice", "app-password")));
    let wrong = WebDavTransport::new(WebDavConfig {
        base_url: stub.base_url.clone(),
        username: "alice".into(),
        password: "wrong-pass".into(),
    })
    .unwrap();

    let assert_auth = |result: crate::error::Result<()>| {
        let err = result.unwrap_err();
        assert!(err.is_code("sync-channel.auth-failed"), "实际: {err:?}");
    };
    assert_auth(wrong.ensure_dir("book-x"));
    assert_auth(wrong.write_file("book-x/manifest.json", b"{}"));
    assert_auth(wrong.read_file("book-x/manifest.json").map(|_| ()));

    // 凭据正确则一切照常（重试成功路径存在）。
    let right = WebDavTransport::new(WebDavConfig {
        base_url: stub.base_url.clone(),
        username: "alice".into(),
        password: "app-password".into(),
    })
    .unwrap();
    right.ensure_dir("book-x").unwrap();
    right.write_file("book-x/manifest.json", b"{}").unwrap();
    assert_eq!(
        right.read_file("book-x/manifest.json").unwrap(),
        Some(b"{}".to_vec())
    );
}

/// 网络失败（连接不可达）：报 `sync-channel.network-failed`（明确、可重试）。
#[test]
fn webdav_unreachable_endpoint_reports_retryable_network_error() {
    // 端口 9（discard）在本测试环境无监听：连接即刻被拒。
    let dav = WebDavTransport::new(WebDavConfig {
        base_url: "http://127.0.0.1:9".into(),
        username: "u".into(),
        password: "p".into(),
    })
    .unwrap();
    let err = dav.read_file("book-x/manifest.json").unwrap_err();
    assert!(err.is_code("sync-channel.network-failed"), "实际: {err:?}");
}

/// 路径守卫：空路径与 `..` 逃逸拒绝（布局规则产出的路径不会触发）。
#[test]
fn webdav_rejects_invalid_paths() {
    let stub = spawn_webdav_stub(None);
    let dav = transport(&stub.base_url);

    for bad in ["", "a/../b", "./x", "a//b"] {
        let err = dav.write_file(bad, b"x").unwrap_err();
        assert!(
            err.is_code("sync-channel.path-invalid"),
            "路径 {bad:?} 应拒绝，实际: {err:?}"
        );
    }
}

/// 空/纯斜杠根地址在构建期即拒绝（凭据与地址是通道配置的两道入门守卫）。
#[test]
fn webdav_empty_base_url_is_rejected() {
    for base in ["", "   ", "///"] {
        let err = WebDavTransport::new(WebDavConfig {
            base_url: base.into(),
            username: "u".into(),
            password: "p".into(),
        })
        .unwrap_err();
        assert!(
            err.is_code("sync-channel.base-url-missing"),
            "实际: {err:?}"
        );
    }
}
