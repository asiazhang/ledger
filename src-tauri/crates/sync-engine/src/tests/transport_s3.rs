use std::sync::Arc;

use crate::transport::Transport;
use crate::transport::s3::{
    AsyncBridge, S3Config, S3Transport, spawn_bridge_thread, transport_runtime,
};
use ledger_infra::error::AppError;
use tauri_app_lib::test_support::{S3Addressing, S3Deny, S3Stub, S3StubConfig, spawn_s3_stub};

fn transport(stub: &S3Stub, prefix: &str) -> S3Transport {
    S3Transport::new(S3Config {
        endpoint: stub.endpoint.clone(),
        region: stub.region.clone(),
        bucket: stub.bucket.clone(),
        access_key: stub.access_key.clone(),
        secret_key: "test-secret".to_string(),
        prefix: prefix.to_string(),
        path_style: stub.addressing == S3Addressing::PathStyle,
    })
    .unwrap()
}

fn path_style_stub() -> S3Stub {
    spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle))
}

#[test]
fn s3_put_get_roundtrip_missing_none_and_prefix_mapping() {
    let stub = path_style_stub();
    let s3 = transport(&stub, "team/ledger");

    let payload: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
    s3.write_file("book-x/streams/d1/seg-0000000001-0000000002.enc", &payload)
        .unwrap();
    assert_eq!(
        s3.read_file("book-x/streams/d1/seg-0000000001-0000000002.enc")
            .unwrap(),
        Some(payload.clone())
    );
    assert!(
        s3.read_file("book-x/streams/d1/absent.enc")
            .unwrap()
            .is_none(),
        "缺对象必须是 None 语义而非错误"
    );

    let put = stub
        .requests()
        .into_iter()
        .find(|request| request.method == "PUT")
        .expect("应观测到 PUT");
    assert_eq!(
        put.path,
        "/ledger-test/team/ledger/book-x/streams/d1/seg-0000000001-0000000002.enc"
    );
    assert!(
        put.header("authorization")
            .is_some_and(|value| value.starts_with("AWS4-HMAC-SHA256 ")),
        "每个请求都必须带 SigV4 Authorization"
    );
}

#[test]
fn s3_config_whitespace_is_normalized_before_signing() {
    let stub = path_style_stub();
    let s3 = S3Transport::new(S3Config {
        endpoint: format!("  {}  ", stub.endpoint),
        region: format!("  {}  ", stub.region),
        bucket: format!("  {}  ", stub.bucket),
        access_key: stub.access_key.clone(),
        secret_key: "test-secret".to_string(),
        prefix: "  team/ledger  ".to_string(),
        path_style: true,
    })
    .unwrap();

    let payload = b"payload".to_vec();
    s3.write_file("book-x/objects/meta.json", &payload).unwrap();
    assert_eq!(
        s3.read_file("book-x/objects/meta.json").unwrap(),
        Some(payload)
    );
}

#[test]
fn s3_ensure_dir_is_side_effect_free_noop() {
    let unreachable = S3Transport::new(S3Config {
        endpoint: "http://127.0.0.1:9".to_string(),
        region: "us-east-1".to_string(),
        bucket: "ledger-test".to_string(),
        access_key: "test-access-key".to_string(),
        secret_key: "test-secret".to_string(),
        prefix: String::new(),
        path_style: true,
    })
    .unwrap();
    unreachable.ensure_dir("book-x/streams/d1").unwrap();

    let stub = path_style_stub();
    let s3 = transport(&stub, "");
    s3.ensure_dir("book-x/streams/d1").unwrap();
    assert!(
        stub.requests().is_empty(),
        "对象存储无目录语义，ensure_dir 不得发请求"
    );
}

#[test]
fn s3_credential_rejection_is_coded_auth_error() {
    let stub = path_style_stub();
    let wrong = S3Transport::new(S3Config {
        endpoint: stub.endpoint.clone(),
        region: stub.region.clone(),
        bucket: stub.bucket.clone(),
        access_key: "wrong-access-key".to_string(),
        secret_key: "test-secret".to_string(),
        prefix: String::new(),
        path_style: true,
    })
    .unwrap();

    let err = wrong.write_file("book-x/manifest.json", b"{}").unwrap_err();
    assert!(err.is_code("sync-channel.auth-failed"), "实际: {err:?}");
    let err = wrong.read_file("book-x/manifest.json").unwrap_err();
    assert!(err.is_code("sync-channel.auth-failed"), "实际: {err:?}");
    assert!(
        !stub.violations().is_empty(),
        "桩应记录凭据范围不一致的结构性违规"
    );
}

#[test]
fn s3_access_denied_is_permission_error_and_missing_bucket_is_target_error() {
    let denied =
        spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle).deny(S3Deny::AccessDenied));
    let s3 = transport(&denied, "");
    let err = s3.read_file("book-x/manifest.json").unwrap_err();
    assert!(
        err.is_code("sync-channel.permission-denied"),
        "AccessDenied 应归「权限不足」而不是「凭据被拒」，实际: {err:?}"
    );

    let missing_bucket =
        spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle).deny(S3Deny::NoSuchBucket));
    let s3 = transport(&missing_bucket, "");
    let err = s3.read_file("book-x/manifest.json").unwrap_err();
    assert!(
        err.is_code("sync-channel.target-missing"),
        "桶不存在的 404 不得被读路径归成「对象不存在」，实际: {err:?}"
    );
    let err = s3.write_file("book-x/manifest.json", b"{}").unwrap_err();
    assert!(err.is_code("sync-channel.target-missing"), "实际: {err:?}");
}

#[test]
fn s3_path_style_and_virtual_host_addressing_are_honored() {
    let path_stub = path_style_stub();
    let path_style = transport(&path_stub, "");
    path_style
        .write_file("book-x/manifest.json", b"{}")
        .unwrap();
    let request = path_stub
        .requests()
        .into_iter()
        .find(|request| request.method == "PUT")
        .expect("path-style 应观测到 PUT");
    assert_eq!(request.path, "/ledger-test/book-x/manifest.json");
    assert!(
        request
            .host
            .as_deref()
            .is_some_and(|host| host.starts_with("127.0.0.1")),
        "path-style 不得把桶放进 Host：{:?}",
        request.host
    );

    let virtual_stub = spawn_s3_stub(S3StubConfig::new(S3Addressing::VirtualHost));
    let virtual_host = transport(&virtual_stub, "");
    virtual_host
        .write_file("book-x/manifest.json", b"{}")
        .unwrap();
    let request = virtual_stub
        .requests()
        .into_iter()
        .find(|request| request.method == "PUT")
        .expect("virtual-host 应观测到 PUT");
    assert_eq!(request.path, "/book-x/manifest.json");
    assert!(
        request
            .host
            .as_deref()
            .is_some_and(|host| host.starts_with("ledger-test.localhost")),
        "virtual-host 形态应把桶放进 Host：{:?}",
        request.host
    );
}

#[test]
fn s3_uploads_omit_aws_checksum_extensions() {
    let stub = path_style_stub();
    let s3 = transport(&stub, "");
    s3.write_file("book-x/manifest.json", b"{}").unwrap();
    let payload = vec![0x5a; 9 * 1024 * 1024];
    s3.write_file("book-x/checkpoint.bin", &payload).unwrap();

    for request in stub.requests() {
        for (name, _) in request.headers {
            assert!(
                !name.starts_with("x-amz-checksum-")
                    && name != "x-amz-sdk-checksum-algorithm"
                    && name != "x-amz-trailer",
                "请求不得携带 AWS 专有校验和扩展: {name}"
            );
        }
    }
}

#[test]
fn s3_large_write_uses_multipart_and_reads_back() {
    let stub = path_style_stub();
    let s3 = transport(&stub, "");
    let payload = vec![0x37; 9 * 1024 * 1024];
    s3.write_file("book-x/checkpoint.bin", &payload).unwrap();
    assert_eq!(
        s3.read_file("book-x/checkpoint.bin").unwrap(),
        Some(payload)
    );

    let requests = stub.requests();
    let creates = requests
        .iter()
        .filter(|request| request.method == "POST" && request.query.as_deref() == Some("uploads"))
        .count();
    let parts = requests
        .iter()
        .filter(|request| {
            request.method == "PUT"
                && request
                    .query
                    .as_deref()
                    .is_some_and(|query| query.contains("partNumber="))
        })
        .count();
    let completes = requests
        .iter()
        .filter(|request| {
            request.method == "POST"
                && request
                    .query
                    .as_deref()
                    .is_some_and(|query| query.contains("uploadId="))
        })
        .count();
    assert_eq!(creates, 1, "应观测到 CreateMultipartUpload");
    assert_eq!(parts, 2, "9 MiB 按 8 MiB 分片应观测到 2 个 UploadPart");
    assert_eq!(completes, 1, "应观测到 CompleteMultipartUpload");
}

#[test]
fn s3_rejects_invalid_paths() {
    let stub = path_style_stub();
    let s3 = transport(&stub, "");
    for bad in ["", "a/../b", "./x", "a//b"] {
        let err = s3.write_file(bad, b"x").unwrap_err();
        assert!(
            err.is_code("sync-channel.path-invalid"),
            "路径 {bad:?} 应拒绝，实际: {err:?}"
        );
    }
    assert!(stub.requests().is_empty(), "非法路径不得触网");
}

#[test]
fn s3_stub_structurally_validates_authorization_scope() {
    let stub = path_style_stub();
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/{}/probe", stub.endpoint, stub.bucket);

    let missing = client.get(&url).send().unwrap();
    assert_eq!(missing.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert!(
        stub.violations()
            .iter()
            .any(|violation| violation.contains("缺少 Authorization")),
        "实际违规: {:?}",
        stub.violations()
    );

    let wrong_region = client
        .get(&url)
        .header(
            reqwest::header::AUTHORIZATION,
            "AWS4-HMAC-SHA256 Credential=test-access-key/20260913/eu-west-1/s3/aws4_request, \
             SignedHeaders=host, Signature=deadbeef",
        )
        .send()
        .unwrap();
    assert_eq!(wrong_region.status(), reqwest::StatusCode::FORBIDDEN);
    let wrong_service = client
        .get(&url)
        .header(
            reqwest::header::AUTHORIZATION,
            "AWS4-HMAC-SHA256 Credential=test-access-key/20260913/us-east-1/ec2/aws4_request, \
             SignedHeaders=host, Signature=deadbeef",
        )
        .send()
        .unwrap();
    assert_eq!(wrong_service.status(), reqwest::StatusCode::FORBIDDEN);
    let violations = stub.violations();
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("region/service")),
        "实际违规: {violations:?}"
    );
}

#[tokio::test]
async fn s3_calls_from_tokio_contexts_do_not_panic() {
    let stub = path_style_stub();
    let s3 = Arc::new(transport(&stub, ""));
    s3.write_file("book-x/manifest.json", b"{}").unwrap();

    let in_blocking = Arc::clone(&s3);
    let read = tokio::task::spawn_blocking(move || {
        in_blocking
            .read_file("book-x/manifest.json")
            .expect("spawn_blocking 内调用不应 panic")
    })
    .await
    .unwrap();
    assert_eq!(read.as_deref(), Some(b"{}".as_slice()));
}

#[tokio::test]
async fn bridge_startup_failure_keeps_caller_thread_free_of_blocking_resource() {
    let failed = AsyncBridge::spawn_with(transport_runtime, |_body| {
        Err(std::io::Error::other("桥接线程不可创建（测试注入）"))
    });
    let Err(err) = failed else {
        panic!("线程创建失败必须在构造点 fail loud 回报调用方");
    };
    assert!(
        err.is_code("sync-channel.network-failed"),
        "线程创建失败沿用既有的网络层可重试错误，实际: {err:?}"
    );
    assert!(
        err.to_string().contains("测试注入"),
        "失败原因应原样回传调用方，实际: {err:?}"
    );
}

#[test]
fn bridge_runtime_build_failure_is_reported_at_construction() {
    let failed = AsyncBridge::spawn_with(
        || {
            Err(AppError::Invalid(
                "运行时初始化失败（测试注入）".to_string(),
            ))
        },
        spawn_bridge_thread,
    );
    let Err(err) = failed else {
        panic!("runtime 构建失败必须在构造点 fail loud 回报调用方");
    };
    assert!(
        err.to_string().contains("测试注入"),
        "构建失败原因应原样回传调用方，实际: {err:?}"
    );
}
