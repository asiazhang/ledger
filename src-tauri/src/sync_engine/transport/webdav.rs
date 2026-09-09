//! WebDAV 通道后端（issue #859 / ADR-0091 决策 1）：v1 内置的哑字节通道实现。
//!
//! 只消费 WebDAV 的最小方法面——`MKCOL`（逐级建目录）、`GET`（读，404 为
//! 常态输入）、`PUT`（整体覆盖写）；不使用锁定与 PROPFIND（目录知识由
//! [`super::super::channel`] 的 manifest 承载，内容自校验兜底弱原子性）。
//! 凭据经 HTTP Basic 随请求发送（WebDAV 网盘通行证形态，如应用密码）；
//! 桌面与移动端走同一实现（reqwest + rustls，ADR-0056 域不依赖壳）。

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

use crate::error::{AppError, Result};

use super::{Transport, auth_failed_error, http_failed_error, network_failed_error};

/// WebDAV 连接配置（凭据与根目录；持久化归 #862 壳层设置面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebDavConfig {
    /// 同步根目录 URL（如 `https://dav.example.com/dav/LedgerSync/`），
    /// 尾部斜杠归一化后拼接逻辑路径。
    pub base_url: String,
    /// WebDAV 账号。
    pub username: String,
    /// WebDAV 密码 / 应用密码。
    pub password: String,
}

/// WebDAV 哑字节通道。可跨线程共享（内部为开销低且同步的 `reqwest` 阻塞客户端）。
#[derive(Debug)]
pub struct WebDavTransport {
    client: Client,
    /// 归一化后的根 URL（无尾斜杠；空路径拒绝）。
    base_url: String,
    auth: HeaderValue,
}

impl WebDavTransport {
    /// 从配置构建通道（连接参数在此定型；不发起网络请求）。
    pub fn new(config: WebDavConfig) -> Result<Self> {
        let base_url = config.base_url.trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            return Err(AppError::coded(
                "sync-channel.base-url-missing",
                "同步通道地址不能为空",
            ));
        }
        let mut headers = HeaderMap::new();
        // 凭据只进请求头，不进日志与错误消息（备份域同纪律：口令不落 trace）。
        let auth = format!("{}:{}", config.username, config.password);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!(
                "Basic {}",
                BASE64_STANDARD.encode(auth.as_bytes())
            ))
            .map_err(|_| {
                AppError::coded(
                    "sync-channel.credentials-invalid",
                    "同步通道账号或密码含非法字符",
                )
            })?,
        );
        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| network_failed_error(&e.to_string()))?;
        Ok(Self {
            client,
            base_url,
            auth: headers[AUTHORIZATION].clone(),
        })
    }

    /// 逻辑路径 → 完整 URL（路径段仅允许布局规则产出的安全字符；`..` 拒绝）。
    fn url_for(&self, path: &str) -> Result<String> {
        if path.is_empty()
            || path
                .split('/')
                .any(|seg| seg.is_empty() || seg == "." || seg == "..")
        {
            return Err(AppError::codedp(
                "sync-channel.path-invalid",
                format!("同步通道路径非法: {path}"),
                &[path],
            ));
        }
        Ok(format!("{}/{path}", self.base_url))
    }

    /// 发送请求并归一错误形态（网络失败 / 凭据被拒 / 其他状态）。
    fn send(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> Result<reqwest::blocking::Response> {
        let request = request.header(AUTHORIZATION, self.auth.clone());
        let response = request.send().map_err(|e| {
            if e.status().is_some() {
                // 有状态码的失败不落这里（send 只在传输层失败时报错），防御归网络错误。
                network_failed_error(&e.to_string())
            } else {
                network_failed_error(&e.to_string())
            }
        })?;
        let status = response.status();
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(auth_failed_error()),
            _ => Ok(response),
        }
    }
}

impl Transport for WebDavTransport {
    fn ensure_dir(&self, path: &str) -> Result<()> {
        // 逐级 MKCOL（WebDAV 无递归建目录）；已存在（405）与成功（201）同为就绪。
        let mut cumulative = String::new();
        for segment in path.split('/') {
            if segment.is_empty() {
                continue;
            }
            if !cumulative.is_empty() {
                cumulative.push('/');
            }
            cumulative.push_str(segment);
            let url = self.url_for(&cumulative)?;
            let response = self.send(self.client.request(mkcol_method(), &url))?;
            let status = response.status();
            match status {
                StatusCode::CREATED | StatusCode::OK | StatusCode::METHOD_NOT_ALLOWED => {}
                _ => return Err(http_failed_error(status.as_u16(), "MKCOL 失败")),
            }
        }
        Ok(())
    }

    fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let url = self.url_for(path)?;
        let response = self.send(self.client.get(&url))?;
        match response.status() {
            StatusCode::OK => Ok(Some(
                response
                    .bytes()
                    .map_err(|e| network_failed_error(&e.to_string()))?
                    .to_vec(),
            )),
            StatusCode::NOT_FOUND => Ok(None),
            status => Err(http_failed_error(status.as_u16(), "GET 失败")),
        }
    }

    fn write_file(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let url = self.url_for(path)?;
        let response = self.send(self.client.put(&url).body(bytes.to_vec()))?;
        match response.status() {
            StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT => Ok(()),
            status => Err(http_failed_error(status.as_u16(), "PUT 失败")),
        }
    }
}

/// `MKCOL` HTTP 方法（WebDAV 扩展方法，按字面量构造）。
fn mkcol_method() -> reqwest::Method {
    reqwest::Method::from_bytes(b"MKCOL").unwrap_or(reqwest::Method::GET)
}
