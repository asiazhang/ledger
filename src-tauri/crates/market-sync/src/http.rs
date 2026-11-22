use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::{Mutex as AsyncMutex, MutexGuard as AsyncMutexGuard};

use ledger_infra::error::{AppError, Result};

const REQUEST_INTERVAL: Duration = Duration::from_millis(1000);
const PACER_SLOWDOWN_FACTOR: u32 = 2;
const PACER_RECOVERY_STEPS: u32 = 10;
const PACER_MAX_INTERVAL: Duration = Duration::from_secs(8);
const MAX_RETRIES: u32 = 3;
const BASE_BACKOFF: Duration = Duration::from_secs(1);
const THROTTLE_COOLDOWN: Duration = Duration::from_secs(30);
const MAX_THROTTLE_RETRIES: u32 = 6;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy)]
pub(super) struct RetryConfig {
    pub(super) max_retries: u32,
    pub(super) base_backoff: Duration,
    pub(super) max_throttle_retries: u32,
    pub(super) throttle_cooldown: Duration,
    pub(super) request_timeout: Duration,
}

impl RetryConfig {
    pub(super) fn production() -> Self {
        Self {
            max_retries: MAX_RETRIES,
            base_backoff: BASE_BACKOFF,
            max_throttle_retries: MAX_THROTTLE_RETRIES,
            throttle_cooldown: THROTTLE_COOLDOWN,
            request_timeout: REQUEST_TIMEOUT,
        }
    }
}

pub(super) struct Pacer {
    last: Option<Instant>,
    interval: Duration,
    baseline: Duration,
}

impl Pacer {
    #[cfg(test)]
    pub(super) fn new(interval: Duration) -> Self {
        Self {
            last: None,
            interval,
            baseline: interval,
        }
    }

    pub(super) async fn wait(&mut self) {
        if let Some(last) = self.last {
            let elapsed = last.elapsed();
            if elapsed < self.interval {
                sleep(self.interval - elapsed).await;
            }
        }
        self.last = Some(Instant::now());
    }

    pub(super) fn interval(&self) -> Duration {
        self.interval
    }

    pub(super) fn record_success(&mut self) {
        if self.interval > self.baseline {
            let step = self.interval / PACER_RECOVERY_STEPS;
            let next = self
                .interval
                .saturating_sub(step.max(Duration::from_millis(1)));
            self.interval = next.max(self.baseline);
        }
    }

    pub(super) fn record_throttled(&mut self) {
        let next = self.interval.saturating_mul(PACER_SLOWDOWN_FACTOR);
        self.interval = next.min(PACER_MAX_INTERVAL.max(self.interval));
    }
}

pub(super) async fn lock_pacer(pacer: &AsyncMutex<Pacer>) -> AsyncMutexGuard<'_, Pacer> {
    pacer.lock().await
}

pub(super) fn shared_pacer() -> Arc<AsyncMutex<Pacer>> {
    static SHARED: OnceLock<Arc<AsyncMutex<Pacer>>> = OnceLock::new();
    SHARED
        .get_or_init(|| {
            Arc::new(AsyncMutex::new(Pacer {
                last: None,
                interval: REQUEST_INTERVAL,
                baseline: REQUEST_INTERVAL,
            }))
        })
        .clone()
}

static FOREGROUND_INFLIGHT: AtomicUsize = AtomicUsize::new(0);

pub(super) struct ForegroundGuard;

impl ForegroundGuard {
    pub(super) fn enter() -> Self {
        FOREGROUND_INFLIGHT.fetch_add(1, Ordering::SeqCst);
        ForegroundGuard
    }
}

impl Drop for ForegroundGuard {
    fn drop(&mut self) {
        FOREGROUND_INFLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

const FOREGROUND_YIELD_POLL: Duration = Duration::from_millis(50);

pub(super) async fn wait_foreground_idle() {
    while FOREGROUND_INFLIGHT.load(Ordering::SeqCst) > 0 {
        sleep(FOREGROUND_YIELD_POLL).await;
    }
}

async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}

pub(super) fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("Mozilla/5.0")
        .build()
        .map_err(|e| AppError::Io(e.to_string()))
}

fn hosts_exhausted_error(failures: &[(String, AppError)]) -> AppError {
    let detail = failures
        .iter()
        .map(|(host, e)| format!("{host}: {e}"))
        .collect::<Vec<_>>()
        .join("; ");
    if let Some((_, coded)) = failures.iter().find(|(_, e)| e.code().is_some()) {
        tracing::warn!(failures = %detail, "全部行情主机请求失败（透传首个码化错误）");
        return coded.clone();
    }
    AppError::coded(
        "market.request-exhausted",
        format!("全部行情主机请求失败: {detail}"),
    )
}
#[allow(clippy::too_many_arguments)]
pub(super) async fn request_json_from_hosts<T>(
    client: &reqwest::Client,
    params: &[(&str, &str)],
    path: &str,
    hosts: &[&str],
    cfg: RetryConfig,
    pacer: &mut Pacer,
    ctx: &str,
    referer: Option<&str>,
) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let mut failures: Vec<(String, AppError)> = Vec::new();
    for host in hosts {
        let url = format!("{host}{path}");
        match request_json_with_retry::<T>(client, &url, params, pacer, ctx, cfg, referer).await {
            Ok(resp) => return Ok(resp),
            Err(e) => failures.push(((*host).to_string(), e)),
        }
    }
    Err(hosts_exhausted_error(&failures))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn request_text_from_hosts(
    client: &reqwest::Client,
    params: &[(&str, &str)],
    path: &str,
    hosts: &[&str],
    cfg: RetryConfig,
    pacer: &mut Pacer,
    ctx: &str,
    referer: Option<&str>,
) -> Result<String> {
    let mut failures: Vec<(String, AppError)> = Vec::new();
    for host in hosts {
        let url = format!("{host}{path}");
        match request_text_with_retry(client, &url, params, pacer, ctx, cfg, referer).await {
            Ok(resp) => return Ok(resp),
            Err(e) => failures.push(((*host).to_string(), e)),
        }
    }
    Err(hosts_exhausted_error(&failures))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn request_bytes_from_hosts(
    client: &reqwest::Client,
    params: &[(&str, &str)],
    path: &str,
    hosts: &[&str],
    cfg: RetryConfig,
    pacer: &mut Pacer,
    ctx: &str,
    referer: Option<&str>,
) -> Result<Vec<u8>> {
    let mut failures: Vec<(String, AppError)> = Vec::new();
    for host in hosts {
        let url = format!("{host}{path}");
        let parsed = request_with_retry(client, &url, params, pacer, ctx, cfg, referer, &|bytes| {
            Ok(bytes.to_vec())
        })
        .await;
        match parsed {
            Ok(resp) => return Ok(resp),
            Err(e) => failures.push(((*host).to_string(), e)),
        }
    }
    Err(hosts_exhausted_error(&failures))
}

pub(super) fn decode_gbk(bytes: &[u8]) -> Result<String> {
    let (text, _, had_errors) = encoding_rs::GBK.decode(bytes);
    if had_errors {
        return Err(AppError::Parse("GBK 解码出错（响应可能被截断）".into()));
    }
    Ok(text.into_owned())
}

pub(super) async fn request_json_with_retry<T>(
    client: &reqwest::Client,
    url: &str,
    params: &[(&str, &str)],
    pacer: &mut Pacer,
    ctx: &str,
    cfg: RetryConfig,
    referer: Option<&str>,
) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    request_with_retry(client, url, params, pacer, ctx, cfg, referer, &|bytes| {
        serde_json::from_slice::<T>(bytes).map_err(|e| format!("JSON 解析失败: {e}"))
    })
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn request_text_with_retry(
    client: &reqwest::Client,
    url: &str,
    params: &[(&str, &str)],
    pacer: &mut Pacer,
    ctx: &str,
    cfg: RetryConfig,
    referer: Option<&str>,
) -> Result<String> {
    request_with_retry(client, url, params, pacer, ctx, cfg, referer, &|bytes| {
        String::from_utf8(bytes.to_vec()).map_err(|e| format!("响应解码失败: {e}"))
    })
    .await
}

#[allow(clippy::too_many_arguments)]
async fn request_with_retry<T, P>(
    client: &reqwest::Client,
    url: &str,
    params: &[(&str, &str)],
    pacer: &mut Pacer,
    ctx: &str,
    cfg: RetryConfig,
    referer: Option<&str>,
    parse: &P,
) -> Result<T>
where
    P: Fn(&[u8]) -> std::result::Result<T, String>,
{
    let mut transport_attempts = 0u32;
    let mut throttle_attempts = 0u32;
    loop {
        pacer.wait().await;
        let mut req = client.get(url).query(params).timeout(cfg.request_timeout);
        if let Some(referer) = referer {
            req = req.header(reqwest::header::REFERER, referer);
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                transport_attempts += 1;
                if transport_attempts <= cfg.max_retries {
                    tracing::warn!(ctx = %ctx, attempt = transport_attempts, error = %e, "HTTP 请求失败，准备重试");
                    sleep(cfg.base_backoff * (1u32 << (transport_attempts - 1))).await;
                    continue;
                }
                tracing::error!(ctx = %ctx, error = %e, "HTTP 请求失败");
                return Err(AppError::Io(format!("HTTP 请求失败: {e}")));
            }
        };

        let status = resp.status();
        let content_encoding = resp
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .map(|v| v.to_str().unwrap_or("?").to_string())
            .unwrap_or_default();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap_or("?").to_string())
            .unwrap_or_default();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            throttle_attempts += 1;
            pacer.record_throttled();
            if throttle_attempts <= cfg.max_throttle_retries {
                tracing::warn!(
                    ctx = %ctx, attempt = throttle_attempts,
                    interval_ms = pacer.interval().as_millis() as u64,
                    "触发接口限流(429)，降速并冷却后重试"
                );
                sleep(cfg.throttle_cooldown).await;
                continue;
            }
            return Err(AppError::coded(
                "market.throttled",
                "接口限流(429)，请稍后再试",
            ));
        }

        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                throttle_attempts += 1;
                if throttle_attempts <= cfg.max_throttle_retries {
                    tracing::warn!(
                        ctx = %ctx, attempt = throttle_attempts, error = %e,
                        "读取响应失败，冷却后重试"
                    );
                    sleep(cfg.throttle_cooldown).await;
                    continue;
                }
                tracing::error!(ctx = %ctx, error = %e, "读取响应失败（重试耗尽）");
                return Err(AppError::Io(format!("读取响应失败: {e}")));
            }
        };
        match parse(&bytes) {
            Ok(parsed) => {
                pacer.record_success();
                return Ok(parsed);
            }
            Err(e) => {
                let head = String::from_utf8_lossy(&bytes[..bytes.len().min(120)]);
                throttle_attempts += 1;
                pacer.record_throttled();
                if throttle_attempts <= cfg.max_throttle_retries {
                    tracing::warn!(
                        ctx = %ctx, attempt = throttle_attempts, status = %status,
                        content_type = %content_type, content_encoding = %content_encoding,
                        interval_ms = pacer.interval().as_millis() as u64,
                        body_head = %head, error = %e,
                        "响应解析失败（疑似被风控拦截），降速并冷却后重试"
                    );
                    sleep(cfg.throttle_cooldown).await;
                    continue;
                }
                tracing::error!(
                    ctx = %ctx, status = %status, content_type = %content_type,
                    content_encoding = %content_encoding, body_head = %head, error = %e,
                    "响应解析失败"
                );
                return Err(AppError::Parse(e));
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct KlineBar {
    pub(super) date: String,
    pub(super) close: f64,
}

impl KlineBar {
    pub fn new(date: impl Into<String>, close: f64) -> Self {
        Self {
            date: date.into(),
            close,
        }
    }
}
