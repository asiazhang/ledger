//! 保存前「测试连接」的读取探针（issue #1219）对真实 S3 桩的行为：
//! 缺对象 = 连通、只读且不列桶、以及凭据 / 权限 / 目标 / 网络四类失败的分层码。
//!
//! 断言对准用户可观察结果（探测结论与用户拿到的码），不断言 SDK 调用形状；
//! 「不要求列桶权限」另以桩观测到的请求面（只有一次对象 GET、无列桶/列对象、
//! 无写请求）作证据——那是判据本身要求的最小权限结论，不是实现形状。

use crate::sync_engine::transport::Transport;
use crate::sync_engine::transport::s3::{S3Config, S3Transport};
use crate::sync_engine::{ChannelBackend, SyncChannelConfig, probe_channel};
use crate::test_support::{S3Addressing, S3Deny, S3StubConfig, spawn_s3_stub};

/// 探针键（同步轮次从不写它；与 `ChannelLayout::probe_path` 同源）。
fn probe_object_path(space: &str) -> String {
    format!("/ledger-test/book-{space}/probe/connectivity")
}

#[test]
fn probe_treats_missing_object_as_connected_and_reads_only() {
    let stub = spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle));
    let config = stub.channel_config("family");

    // 桶是新开的、什么对象都没有：取不到探针对象恰恰是连通形态。
    probe_channel(&config).expect("缺对象必须是「连通」而不是失败");

    let requests = stub.requests();
    assert_eq!(requests.len(), 1, "探针应只发一次请求：{requests:?}");
    let request = &requests[0];
    assert_eq!(request.method, "GET", "探针必须是对象读取，不是写入或列桶");
    assert_eq!(
        request.path,
        probe_object_path("family"),
        "探针对象应落在同步空间目录下（权限范围与真实轮次一致）"
    );
    // 最小权限子账号通常没有 ListBucket：探针不得走列桶/列对象，也不得留写痕迹。
    assert!(
        request
            .query
            .as_deref()
            .is_none_or(|query| !query.contains("list-type")),
        "探针不得列对象：{:?}",
        request.query
    );
    assert!(
        !stub
            .requests()
            .iter()
            .any(|r| r.method != "GET" || r.path == "/"),
        "探针只应有对象 GET，不得有写请求或列桶请求"
    );

    // 探针对象真的存在时同样连通（例如保留键恰被别的工具放过东西）：探测结论
    // 不取决于对象在不在，只取决于「读得动」。
    let transport = S3Transport::new(S3Config {
        endpoint: config.endpoint.clone(),
        region: config.region.clone(),
        bucket: config.bucket.clone(),
        access_key: config.access_key.clone(),
        secret_key: config.secret_key.clone(),
        prefix: config.prefix.clone(),
        path_style: config.path_style,
    })
    .expect("构库应成功");
    transport
        .write_file("book-family/probe/connectivity", b"probe")
        .expect("写入探针对象应成功");
    probe_channel(&config).expect("探针对象存在时同样连通");
}

#[test]
fn probe_reports_credentials_rejected_for_invalid_access_key() {
    let stub =
        spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle).deny(S3Deny::InvalidAccessKeyId));
    let err = probe_channel(&stub.channel_config("family")).expect_err("无效密钥应被拒");
    assert!(err.is_code("sync-channel.auth-failed"), "实际: {err:?}");
}

#[test]
fn probe_reports_permission_denied_for_access_denied() {
    let stub = spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle).deny(S3Deny::AccessDenied));
    let err = probe_channel(&stub.channel_config("family")).expect_err("无权访问应被拒");
    assert!(
        err.is_code("sync-channel.permission-denied"),
        "403 + AccessDenied 应归「权限不足」而不是「凭据被拒」，实际: {err:?}"
    );
}

#[test]
fn probe_reports_target_missing_for_missing_bucket() {
    let stub = spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle).deny(S3Deny::NoSuchBucket));
    let err = probe_channel(&stub.channel_config("family")).expect_err("桶不存在应报目标不存在");
    assert!(
        err.is_code("sync-channel.target-missing"),
        "404 + NoSuchBucket 不得被当成「通道上没有数据」，实际: {err:?}"
    );
}

#[test]
fn probe_reports_network_failure_for_unreachable_endpoint() {
    let config = SyncChannelConfig {
        backend: ChannelBackend::S3,
        endpoint: "http://127.0.0.1:1".to_string(),
        region: "us-east-1".to_string(),
        bucket: "ledger-test".to_string(),
        access_key: "test-access-key".to_string(),
        secret_key: "test-secret".to_string(),
        space_id: "family".to_string(),
        ..Default::default()
    };
    let err = probe_channel(&config).expect_err("连不上的端点应报网络失败");
    assert!(err.is_code("sync-channel.network-failed"), "实际: {err:?}");
}
