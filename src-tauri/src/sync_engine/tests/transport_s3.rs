//! S3 兼容对象存储通道后端（issue #1216 / ADR-0091 决策 1）对本地 S3 桩的
//! 读写行为、错误归一（缺对象 = 不存在、凭据拒绝 = 码化错误）、自定义端点与
//! 寻址形态、兼容优先校验和策略、分片上传与同步 → 异步桥接。

use std::sync::Arc;

use crate::sync_engine::transport::Transport;
use crate::sync_engine::transport::s3::{S3Config, S3Transport};
use crate::test_support::{S3Addressing, S3Stub, S3StubConfig, spawn_s3_stub};

/// 按桩配置构造可用传输（path-style / virtual-host 随桩）。
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

/// PUT → GET 往返逐字节一致；缺对象回 None；对象键带配置前缀。
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

/// ensure_dir 是零副作用空操作：端点不可达也返回 Ok，桩上零请求。
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

/// 凭据范围不匹配：桩回 403，后端归一为可重试的 `sync-channel.auth-failed`。
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

/// 自定义端点 + path-style / virtual-host 开关都落到请求寻址形态上。
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

/// 兼容优先：上传请求不得追加 AWS 专有校验和扩展（显式 WhenRequired）。
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

/// 大文件自动分片：桩观测到 Create / UploadPart×N / Complete，读回内容一致。
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

/// 路径守卫：空路径、空段、`.` 与 `..` 逃逸拒绝。
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

/// 桩结构性校验：缺 Authorization = 401；region / service 与配置不符 = 403。
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

/// 调用方已处在 Tokio runtime / spawn_blocking 上下文时调用也不 panic：
/// 后端自持专用 runtime，不用调用方线程 block_on。
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
