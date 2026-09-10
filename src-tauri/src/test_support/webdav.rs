//! 共享 WebDAV 测试桩（issue #862 上收统一测试工厂，ADR-0084 准入：多端同步域
//! 域单测与命令面集成测试 ≥2 处同体消费）：axum 实现的最小 WebDAV 服务——
//! `MKCOL` / `GET` / `PUT` 三方法 + 可选 Basic 认证，临时目录承载文件，供
//! WebDAV 后端、通道轮次与「双端经真实通道同步」集成测试使用。
//!
//! **可见性**：`pub` + `#[doc(hidden)]`（本模块同款纪律）——集成测试链接非
//! `#[cfg(test)]` 构建的 lib，经 `crate::test_support` 消费。
// C 类豁免（ADR-0060）：仅测试用——桩体大量 unwrap 依赖测试期失败即红的语义，
// 本文件随 test_support 文件级放行六件套（见 mod.rs 豁免声明）。

/// 本地 WebDAV 桩句柄：同步根 URL + 临时根目录（Drop 清理）。
pub struct WebDavStub {
    /// 同步根 URL（`http://127.0.0.1:<port>`）。
    pub base_url: String,
    root: std::path::PathBuf,
}

impl Drop for WebDavStub {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

/// 起一个本地 WebDAV 桩；`auth` 为 `Some((user, pass))` 时要求 Basic 认证
/// （凭据错误 → 401）。返回桩句柄与独立临时根目录。
pub fn spawn_webdav_stub(auth: Option<(&str, &str)>) -> WebDavStub {
    use axum::extract::{Request, State};
    use axum::http::{HeaderName, StatusCode};
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;

    struct StubState {
        root: std::path::PathBuf,
        /// 期望的 Authorization 头完整值（`Basic <base64>`）；None = 不鉴权。
        expected_auth: Option<String>,
    }

    /// MKCOL 扩展方法（http crate 无常量，按字面量解析）。
    fn mkcol() -> axum::http::Method {
        axum::http::Method::from_bytes(b"MKCOL").unwrap()
    }

    /// 统一入口：按 HTTP 方法分发 WebDAV 最小语义（响应主体为原始字节）。
    async fn dav(
        State(state): State<std::sync::Arc<StubState>>,
        request: Request,
    ) -> (StatusCode, Vec<u8>) {
        if let Some(expected) = &state.expected_auth {
            let got = request
                .headers()
                .get(HeaderName::from_static("authorization"))
                .and_then(|v| v.to_str().ok());
            if got != Some(expected.as_str()) {
                return (StatusCode::UNAUTHORIZED, Vec::new());
            }
        }
        let path = request.uri().path().trim_start_matches('/').to_string();
        if path
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == "..")
        {
            return (StatusCode::BAD_REQUEST, Vec::new());
        }
        let target = state.root.join(&path);
        let method = request.method().clone();
        if method == mkcol() {
            if target.is_dir() {
                return (StatusCode::METHOD_NOT_ALLOWED, Vec::new());
            }
            return match std::fs::create_dir_all(&target) {
                Ok(()) => (StatusCode::CREATED, Vec::new()),
                Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Vec::new()),
            };
        }
        if method == axum::http::Method::PUT {
            let body = axum::body::to_bytes(request.into_body(), usize::MAX)
                .await
                .unwrap_or_default();
            let parent = target.parent().unwrap_or(&state.root).to_path_buf();
            return match (
                std::fs::create_dir_all(&parent),
                std::fs::write(&target, &body),
            ) {
                (Ok(()), Ok(())) => (StatusCode::CREATED, Vec::new()),
                _ => (StatusCode::INTERNAL_SERVER_ERROR, Vec::new()),
            };
        }
        if method == axum::http::Method::GET {
            return match std::fs::read(&target) {
                Ok(bytes) => (StatusCode::OK, bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    (StatusCode::NOT_FOUND, Vec::new())
                }
                Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Vec::new()),
            };
        }
        (StatusCode::METHOD_NOT_ALLOWED, Vec::new())
    }

    let root = std::env::temp_dir().join(format!("ledger-webdav-stub-{}", crate::db::new_uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let expected_auth =
        auth.map(|(user, pass)| format!("Basic {}", B64.encode(format!("{user}:{pass}"))));
    let app = axum::Router::new()
        .route(
            "/{*path}",
            axum::routing::any(dav).with_state(std::sync::Arc::new(StubState {
                root: root.clone(),
                expected_auth,
            })),
        )
        .route(
            "/",
            axum::routing::any(|| async { StatusCode::METHOD_NOT_ALLOWED }),
        );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            // tokio 新版要求 fd 先行置为非阻塞再注册（issue #7172）。
            listener.set_nonblocking(true).unwrap();
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
    WebDavStub {
        base_url: format!("http://{addr}"),
        root,
    }
}
