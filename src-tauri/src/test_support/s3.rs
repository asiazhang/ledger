//! 共享 S3 测试桩（issue #1216 / ADR-0084 准入：多端同步域 S3 后端单测与
//! 通道轮次域单测 ≥2 处同体消费）：axum 实现的最小 S3 兼容服务——PUT / GET /
//! CreateMultipartUpload / UploadPart / CompleteMultipartUpload /
//! AbortMultipartUpload，临时目录承载对象与分片，供 S3 后端与真实 HTTP 同步
//! 轮次使用。
//!
//! **结构性校验，不做密码学校验**：Authorization 头必须在场，凭据范围里的
//! region / service 必须与桩配置一致；寻址形态（path-style / virtual-host）
//! 必须与配置一致。签名正确性归官方 SDK，本桩只验证请求骨架。
//!
//! **可见性**：`pub` + `#[doc(hidden)]`（与 `webdav` 同款纪律）——集成测试链接
//! 非 `#[cfg(test)]` 构建的 lib，经 `crate::test_support` 消费。
// C 类豁免（ADR-0060）：仅测试用——桩体大量 unwrap 依赖测试期失败即红的语义，
// 本文件随 test_support 文件级放行六件套（见 mod.rs 豁免声明）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 桩期望的 S3 寻址形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3Addressing {
    /// `/{bucket}/{key}`，监听 `127.0.0.1`。
    PathStyle,
    /// `Host: {bucket}.localhost` + `/{key}`，监听 `[::1]`（macOS 上 localhost
    /// 解析到 `::1`）。
    VirtualHost,
}

/// S3 桩配置（默认值适用于多数用例；凭据范围用 access key 校验归属）。
#[derive(Debug, Clone)]
pub struct S3StubConfig {
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub addressing: S3Addressing,
}

impl S3StubConfig {
    pub fn new(addressing: S3Addressing) -> Self {
        Self {
            region: "us-east-1".to_string(),
            bucket: "ledger-test".to_string(),
            access_key: "test-access-key".to_string(),
            addressing,
        }
    }
}

/// 桩观测到的一次 HTTP 请求（方法 / 路径 / 查询串 / Host / 请求头）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3ObservedRequest {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub host: Option<String>,
    pub headers: Vec<(String, String)>,
}

impl S3ObservedRequest {
    /// 按小写头名读取观测到的请求头。
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// 本地 S3 桩句柄：端点 + 临时根目录（Drop 清理）。
pub struct S3Stub {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub addressing: S3Addressing,
    root: std::path::PathBuf,
    observations: Arc<Mutex<Vec<S3ObservedRequest>>>,
    violations: Arc<Mutex<Vec<String>>>,
}

impl S3Stub {
    pub fn requests(&self) -> Vec<S3ObservedRequest> {
        self.observations.lock().unwrap().clone()
    }

    pub fn violations(&self) -> Vec<String> {
        self.violations.lock().unwrap().clone()
    }

    pub fn clear_requests(&self) {
        self.observations.lock().unwrap().clear();
    }
}

impl Drop for S3Stub {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

struct StubState {
    root: std::path::PathBuf,
    region: String,
    bucket: String,
    access_key: String,
    addressing: S3Addressing,
    uploads: Mutex<HashMap<String, (String, String)>>,
    observations: Arc<Mutex<Vec<S3ObservedRequest>>>,
    violations: Arc<Mutex<Vec<String>>>,
}

fn xml_response(status: axum::http::StatusCode, body: String) -> axum::response::Response {
    let mut response = axum::response::Response::new(axum::body::Body::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/xml"),
    );
    response
}

fn empty_response(status: axum::http::StatusCode) -> axum::response::Response {
    let mut response = axum::response::Response::new(axum::body::Body::empty());
    *response.status_mut() = status;
    response
}

fn etag_response(status: axum::http::StatusCode, etag: &str) -> axum::response::Response {
    let mut response = empty_response(status);
    response.headers_mut().insert(
        axum::http::header::ETAG,
        axum::http::HeaderValue::from_str(etag).unwrap(),
    );
    response
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| match part.split_once('=') {
            Some((key, value)) => (key.to_string(), value.to_string()),
            None => (part.to_string(), String::new()),
        })
        .collect()
}

/// 解析 SigV4 Authorization 的 Credential 范围；形态不符回 None。
fn credential_scope(authorization: &str) -> Option<(String, String, String)> {
    let rest = authorization.strip_prefix("AWS4-HMAC-SHA256 ")?;
    if !rest.contains("SignedHeaders=") || !rest.contains("Signature=") {
        return None;
    }
    let credential = rest.split(',').next()?.trim().strip_prefix("Credential=")?;
    let mut parts = credential.split('/');
    let access_key = parts.next()?.to_string();
    let _date = parts.next()?;
    let region = parts.next()?.to_string();
    let service = parts.next()?.to_string();
    if parts.next()? != "aws4_request" || parts.next().is_some() {
        return None;
    }
    Some((access_key, region, service))
}

/// 按桩配置从请求中解析 bucket 与对象键，同时校验寻址形态。
fn addressing_key(state: &StubState, path: &str, host: Option<&str>) -> Result<String, String> {
    match state.addressing {
        S3Addressing::PathStyle => {
            if host
                .and_then(|h| h.split(':').next())
                .is_some_and(|h| h.starts_with(&format!("{}.", state.bucket)))
            {
                return Err(format!(
                    "path-style 配置收到 virtual-host 形态 Host: {host:?}"
                ));
            }
            let trimmed = path.trim_start_matches('/');
            let (bucket, key) = trimmed
                .split_once('/')
                .ok_or_else(|| format!("path-style 请求缺少对象键: {path}"))?;
            if bucket != state.bucket {
                return Err(format!(
                    "path-style 请求的桶与配置不一致: {bucket:?} != {:?}",
                    state.bucket
                ));
            }
            Ok(key.to_string())
        }
        S3Addressing::VirtualHost => {
            let hostname = host
                .and_then(|h| h.split(':').next())
                .ok_or_else(|| "virtual-host 请求缺少 Host".to_string())?;
            let expected = format!("{}.localhost", state.bucket);
            if hostname != expected {
                return Err(format!(
                    "virtual-host 形态与配置不一致: {hostname:?} != {expected:?}"
                ));
            }
            let key = path.trim_start_matches('/');
            if key.is_empty() {
                return Err("virtual-host 请求缺少对象键".to_string());
            }
            Ok(key.to_string())
        }
    }
}

fn object_path(state: &StubState, bucket: &str, key: &str) -> std::path::PathBuf {
    state.root.join("objects").join(bucket).join(key)
}

fn multipart_dir(state: &StubState, upload_id: &str) -> std::path::PathBuf {
    state.root.join("multipart").join(upload_id)
}

/// 起一个本地 S3 桩；寻址形态决定监听地址（见 [`S3Addressing`]）。
pub fn spawn_s3_stub(config: S3StubConfig) -> S3Stub {
    use axum::extract::{Request, State};
    use axum::http::{Method, StatusCode};

    async fn s3(State(state): State<Arc<StubState>>, request: Request) -> axum::response::Response {
        let method = request.method().clone();
        let uri = request.uri().clone();
        let host = request
            .headers()
            .get(axum::http::header::HOST)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let headers: Vec<(String, String)> = request
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_ascii_lowercase(),
                    value.to_str().unwrap_or("<binary>").to_string(),
                )
            })
            .collect();
        let authorization = request
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        state.observations.lock().unwrap().push(S3ObservedRequest {
            method: method.as_str().to_string(),
            path: uri.path().to_string(),
            query: uri.query().map(str::to_string),
            host: host.clone(),
            headers,
        });

        let Some(authorization) = authorization else {
            state.violations.lock().unwrap().push(format!(
                "缺少 Authorization 头: {} {}",
                method,
                uri.path()
            ));
            return empty_response(StatusCode::UNAUTHORIZED);
        };
        let Some((access_key, region, service)) = credential_scope(&authorization) else {
            state
                .violations
                .lock()
                .unwrap()
                .push(format!("Authorization 形态非法: {authorization}"));
            return empty_response(StatusCode::FORBIDDEN);
        };
        if access_key != state.access_key {
            state
                .violations
                .lock()
                .unwrap()
                .push(format!("凭据范围 access key 不一致: {access_key:?}"));
            return empty_response(StatusCode::FORBIDDEN);
        }
        if region != state.region || service != "s3" {
            state.violations.lock().unwrap().push(format!(
                "凭据范围 region/service 不一致: {region}/{service} != {}/s3",
                state.region
            ));
            return empty_response(StatusCode::FORBIDDEN);
        }

        let key = match addressing_key(&state, uri.path(), host.as_deref()) {
            Ok(key) => key,
            Err(violation) => {
                state.violations.lock().unwrap().push(violation);
                return empty_response(StatusCode::BAD_REQUEST);
            }
        };
        let query = parse_query(uri.query().unwrap_or(""));
        let body = axum::body::to_bytes(request.into_body(), usize::MAX)
            .await
            .unwrap_or_default();

        if method == Method::PUT {
            if let (Some(upload_id), Some(part_number)) =
                (query.get("uploadId"), query.get("partNumber"))
            {
                let expected = state.uploads.lock().unwrap().get(upload_id).cloned();
                if expected.as_ref() != Some(&(state.bucket.clone(), key.clone())) {
                    return empty_response(StatusCode::NOT_FOUND);
                }
                let dir = multipart_dir(&state, upload_id);
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join(part_number), &body).unwrap();
                return etag_response(StatusCode::OK, &format!("\"part-{part_number}\""));
            }

            let target = object_path(&state, &state.bucket, &key);
            let parent = target.parent().unwrap().to_path_buf();
            std::fs::create_dir_all(&parent).unwrap();
            std::fs::write(&target, &body).unwrap();
            return etag_response(StatusCode::OK, "\"put-object\"");
        }

        if method == Method::GET {
            return match std::fs::read(object_path(&state, &state.bucket, &key)) {
                Ok(bytes) => {
                    let mut response = axum::response::Response::new(axum::body::Body::from(bytes));
                    response.headers_mut().insert(
                        axum::http::header::CONTENT_TYPE,
                        axum::http::HeaderValue::from_static("application/octet-stream"),
                    );
                    response
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => xml_response(
                    StatusCode::NOT_FOUND,
                    format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                         <Error><Code>NoSuchKey</Code><Message>NoSuchKey</Message>\
                         <Key>{key}</Key></Error>"
                    ),
                ),
                Err(_) => empty_response(StatusCode::INTERNAL_SERVER_ERROR),
            };
        }

        if method == Method::POST {
            if query.contains_key("uploads") {
                let upload_id = crate::db::new_uuid();
                state
                    .uploads
                    .lock()
                    .unwrap()
                    .insert(upload_id.clone(), (state.bucket.clone(), key.clone()));
                std::fs::create_dir_all(multipart_dir(&state, &upload_id)).unwrap();
                return xml_response(
                    StatusCode::OK,
                    format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                         <InitiateMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
                         <Bucket>{}</Bucket><Key>{key}</Key><UploadId>{upload_id}</UploadId>\
                         </InitiateMultipartUploadResult>",
                        state.bucket
                    ),
                );
            }
            if let Some(upload_id) = query.get("uploadId") {
                let expected = state.uploads.lock().unwrap().get(upload_id).cloned();
                if expected.as_ref() != Some(&(state.bucket.clone(), key.clone())) {
                    return empty_response(StatusCode::NOT_FOUND);
                }
                let dir = multipart_dir(&state, upload_id);
                let mut parts: Vec<(u64, std::path::PathBuf)> = std::fs::read_dir(&dir)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .map(|path| {
                        let number = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .and_then(|n| n.parse::<u64>().ok())
                            .unwrap();
                        (number, path)
                    })
                    .collect();
                parts.sort_by_key(|(number, _)| *number);
                let mut assembled = Vec::new();
                for (_, path) in parts {
                    assembled.extend_from_slice(&std::fs::read(path).unwrap());
                }
                let target = object_path(&state, &state.bucket, &key);
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                std::fs::write(&target, &assembled).unwrap();
                std::fs::remove_dir_all(&dir).ok();
                state.uploads.lock().unwrap().remove(upload_id);
                return xml_response(
                    StatusCode::OK,
                    format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                         <CompleteMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
                         <Location>{}</Location><Bucket>{}</Bucket><Key>{key}</Key><ETag>\"complete\"</ETag>\
                         </CompleteMultipartUploadResult>",
                        state.bucket, state.bucket
                    ),
                );
            }
        }

        if method == Method::DELETE
            && let Some(upload_id) = query.get("uploadId")
        {
            state.uploads.lock().unwrap().remove(upload_id);
            std::fs::remove_dir_all(multipart_dir(&state, upload_id)).ok();
            return empty_response(StatusCode::NO_CONTENT);
        }

        empty_response(StatusCode::METHOD_NOT_ALLOWED)
    }

    let root = std::env::temp_dir().join(format!("ledger-s3-stub-{}", crate::db::new_uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let observations = Arc::new(Mutex::new(Vec::new()));
    let violations = Arc::new(Mutex::new(Vec::new()));
    let state = Arc::new(StubState {
        root: root.clone(),
        region: config.region.clone(),
        bucket: config.bucket.clone(),
        access_key: config.access_key.clone(),
        addressing: config.addressing,
        uploads: Mutex::new(HashMap::new()),
        observations: observations.clone(),
        violations: violations.clone(),
    });
    let app = axum::Router::new()
        .route("/{*path}", axum::routing::any(s3).with_state(state))
        .route(
            "/",
            axum::routing::any(|| async { axum::http::StatusCode::METHOD_NOT_ALLOWED }),
        );
    let bind = match config.addressing {
        S3Addressing::PathStyle => "127.0.0.1:0",
        S3Addressing::VirtualHost => "[::1]:0",
    };
    let listener = std::net::TcpListener::bind(bind).unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            listener.set_nonblocking(true).unwrap();
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
    let endpoint = match config.addressing {
        S3Addressing::PathStyle => format!("http://{addr}"),
        S3Addressing::VirtualHost => format!("http://localhost:{}", addr.port()),
    };
    S3Stub {
        endpoint,
        region: config.region,
        bucket: config.bucket,
        access_key: config.access_key,
        addressing: config.addressing,
        root,
        observations,
        violations,
    }
}
