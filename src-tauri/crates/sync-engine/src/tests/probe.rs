use tauri_app_lib::ledger_sync_engine::transport::Transport;
use tauri_app_lib::ledger_sync_engine::transport::s3::{S3Config, S3Transport};
use tauri_app_lib::ledger_sync_engine::{SyncChannelConfig, probe_channel};
use tauri_app_lib::test_support::{S3Addressing, S3Deny, S3StubConfig, spawn_s3_stub};

fn probe_object_path(space: &str) -> String {
    format!("/ledger-test/book-{space}/probe/connectivity")
}

#[test]
fn probe_treats_missing_object_as_connected_and_reads_only() {
    let stub = spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle));
    let config = stub.channel_config("family");

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
