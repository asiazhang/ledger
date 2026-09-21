//! 行情 HTTP 网络层（issue #89）：行情接口请求、多主机切换、重试与限流冷却、
//! 响应解析。与数据库无关，可独立测试（见 `tests.rs` 中本地 HTTP 服务用例）。
//! 客户端与等待原语为异步形态（reqwest async / 异步睡眠 / 异步互斥体，issue #1411
//! / ADR-0125 决策 5/6）；同步编排与通道束闭包已随 #1412 直接 `.await`，两壳生产
//! 入口与投资域注入闭包已随 #1413 async 化，#1411 过渡同步桥拆除——本层生产面
//! 不再有任何阻塞驱动点。
//! 标的全量同步（clist 分页爬取）已随 ADR-0081 决策 3 退役删除（issue #698），
//! 单点行情（stock/get）已随 #1567 接线腾讯后删除、历史净值通道（lsjz）已随
//! #1571 接线新浪全历史面后删除、东财日 K / FX 汇率腿已随 #1551 换 ECB 后删除
//!（ADR-0130 决策 1：不留死代码）。本层是多主机轮换 / 重试 / 限流冷却与 GBK
//! 解码的共享原语，供现役取数单元消费：腾讯行情批量报价（字节 + GBK）、腾讯
//! 日 K（JSON）、新浪场外基金批量面与单只全历史面（字节 / 文本）、基金详情
//! `.js` 数据文件（文本）、证监会基金电子披露（文本）与 ECB 参考汇率文件（文本）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::{Mutex as AsyncMutex, MutexGuard as AsyncMutexGuard};

use ledger_infra::error::{AppError, Result};

// 共享限速基线（ADR-0121 决策 5）：相邻两次请求至少间隔 1 秒——「正常状态贴近
// 数据源可承受量级」的保守起点，东财时代标定后沿用为全部分享通道的共用节奏
//（现役：腾讯 / 新浪 / 证监会披露 / ECB）。写死的固定间隔在两个方向上都错
//（保守值浪费额度、激进值撞风控），故限速随观测自适应：命中限流/拦截页即降速
// 并复用既有冷却，之后每成功一次逐步回升到本起点。
const REQUEST_INTERVAL: Duration = Duration::from_millis(1000);
/// 自适应限速的降速倍数（命中限流 / 疑似风控页时）。
const PACER_SLOWDOWN_FACTOR: u32 = 2;
/// 自适应限速的回升步长：每次成功请求把间隔降 10%（不低过起点）。
const PACER_RECOVERY_STEPS: u32 = 10;
/// 自适应限速的上限（连续被限流时的最大请求间隔）。
const PACER_MAX_INTERVAL: Duration = Duration::from_secs(8);
const MAX_RETRIES: u32 = 3;
const BASE_BACKOFF: Duration = Duration::from_secs(1);
const THROTTLE_COOLDOWN: Duration = Duration::from_secs(30);
const MAX_THROTTLE_RETRIES: u32 = 6;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// 重试策略：传输层错误走短退避，风控限流（429 / 200 非 JSON）走长冷却等待窗口过去。
#[derive(Clone, Copy)]
pub(super) struct RetryConfig {
    pub(super) max_retries: u32,
    pub(super) base_backoff: Duration,
    pub(super) max_throttle_retries: u32,
    pub(super) throttle_cooldown: Duration,
}

impl RetryConfig {
    pub(super) fn production() -> Self {
        Self {
            max_retries: MAX_RETRIES,
            base_backoff: BASE_BACKOFF,
            max_throttle_retries: MAX_THROTTLE_RETRIES,
            throttle_cooldown: THROTTLE_COOLDOWN,
        }
    }
}

/// 串行自适应限速器（ADR-0121 决策 5）：保证相邻两次 HTTP 请求之间至少间隔当前
/// 间隔，间隔随观测自适应——正常状态停在构造时的基线（生产 = 数据源可承受量级），
/// 命中限流响应或疑似风控页翻倍降速（上限 [`PACER_MAX_INTERVAL`]），此后每成功
/// 一次逐步回升（[`PACER_RECOVERY_STEPS`] 步回到基线）。
///
/// 基线即构造间隔，也是回升下限与降速下限的锚：测试传零间隔即整只限速器惰性
///（降速乘零仍是零），不因自适应逻辑凭空产生等待。
pub(super) struct Pacer {
    last: Option<Instant>,
    interval: Duration,
    /// 基线间隔（正常状态的目标值）：回升不低过它、降速以它为起点。
    baseline: Duration,
}

impl Pacer {
    pub(super) fn new(interval: Duration) -> Self {
        Self {
            last: None,
            interval,
            baseline: interval,
        }
    }

    /// 发起一次请求前的等待：不足当前间隔则异步睡满（ADR-0125 决策 6——等待
    /// 原语异步化，间隔与串行保证不变）。
    pub(super) async fn wait(&mut self) {
        if let Some(last) = self.last {
            let elapsed = last.elapsed();
            if elapsed < self.interval {
                sleep(self.interval - elapsed).await;
            }
        }
        self.last = Some(Instant::now());
    }

    /// 当前请求间隔（观测与测试用）：限速自适应的可观察面。
    pub(super) fn interval(&self) -> Duration {
        self.interval
    }

    /// 一次成功请求（响应按预期解析）：间隔逐步回升到基线——
    /// 「恢复后逐步回升」，不一步跳回（避免风控窗口刚过就重新撞上）。
    pub(super) fn record_success(&mut self) {
        if self.interval > self.baseline {
            let step = self.interval / PACER_RECOVERY_STEPS;
            let next = self
                .interval
                .saturating_sub(step.max(Duration::from_millis(1)));
            self.interval = next.max(self.baseline);
        }
    }

    /// 命中限流响应（429）或疑似风控页（非 JSON 拦截页）：翻倍降速（封顶）。
    /// 冷却等待复用 `RetryConfig::throttle_cooldown`（本处只调间隔）。
    pub(super) fn record_throttled(&mut self) {
        let next = self.interval.saturating_mul(PACER_SLOWDOWN_FACTOR);
        self.interval = next.min(PACER_MAX_INTERVAL.max(self.interval));
    }
}

impl Default for Pacer {
    fn default() -> Self {
        Self::new(REQUEST_INTERVAL)
    }
}

/// 取一次限流 pacer 的异步守卫：从发请求前一直持有到响应处理完（ADR-0125
/// 决策 6——锁的粒度不变，只把 `std` 互斥体换成可在 `.await` 之间持有的异步
/// 互斥体；`std` 守卫跨 `await` 持有会让 future 失去 `Send`）。异步互斥体无
/// 中毒语义：抓取 panic 不再毒化限速状态。
pub(super) async fn lock_pacer(pacer: &AsyncMutex<Pacer>) -> AsyncMutexGuard<'_, Pacer> {
    pacer.lock().await
}

/// 进程级共享限速器单例（issue #1375 额度让路）：前台（用户动作：手动同步）与
/// 后台补全两条通道束共享同一份 `Pacer`——数据源的请求额度是进程全局的，
/// 相邻两次请求（不管来自哪条车道）之间都保持当前间隔。先例：
/// `bulk::shared_circuit`（跨同步记忆的进程级单例，通道束每次重建而记忆不随束
/// 消亡）；限速器同理——束每次同步/每轮重建，限速状态必须活在束之外。
pub(super) fn shared_pacer() -> Arc<AsyncMutex<Pacer>> {
    static SHARED: OnceLock<Arc<AsyncMutex<Pacer>>> = OnceLock::new();
    SHARED
        .get_or_init(|| Arc::new(AsyncMutex::new(Pacer::default())))
        .clone()
}

/// 前台在途计数（issue #1375 让行语义的状态位）：前台车道每个请求在途期间
/// 持有一个 [`ForegroundGuard`]，计数即「此刻有前台请求在途」；后台车道发
/// 请求前等它归零。原子量足够：让行是尽力而为的礼让语义，硬保证由共享
/// pacer 的互斥串行承担。
static FOREGROUND_INFLIGHT: AtomicUsize = AtomicUsize::new(0);

/// 前台在途守卫（RAII）：构造即计数 +1，作用域结束（请求完成/失败/panic）
/// 自动 -1。前台车道的抓取闭包在请求前构造，闭包返回时自然释放。
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

/// 后台让行等待（issue #1375）：前台请求在途期间异步轮询等待归零，绝不与前台
/// 并发抢额度——用户动作优先于后台补全。归零窗口极短（前台请求本身被共享
/// pacer 限速），轮询间隔取小让后台能及时察觉恢复。
const FOREGROUND_YIELD_POLL: Duration = Duration::from_millis(50);

pub(super) async fn wait_foreground_idle() {
    while FOREGROUND_INFLIGHT.load(Ordering::SeqCst) > 0 {
        sleep(FOREGROUND_YIELD_POLL).await;
    }
}

/// 异步睡眠原语（ADR-0125 决策 6）：限速等待、退避与让行自旋统一走这里，
/// 不占用调用线程。
async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}

/// 构建行情 HTTP 客户端（异步 reqwest，issue #1411 / ADR-0125 决策 5：不再自持
/// 运行时线程，构造与请求等待都不再要求调用线程不在异步上下文；增量同步与
/// 按代码查询通道共用，UA 保持一致）。
pub(super) fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("Mozilla/5.0")
        .build()
        .map_err(|e| AppError::Io(e.to_string()))
}

/// 发送请求并解析 JSON，按序尝试多个主机，对传输错误做短退避、对限流拦截做长冷却重试。
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
    let mut failures: Vec<String> = Vec::new();
    for host in hosts {
        let url = format!("{host}{path}");
        match request_json_with_retry::<T>(client, &url, params, pacer, ctx, cfg, referer).await {
            Ok(resp) => return Ok(resp),
            Err(e) => failures.push(format!("{host}: {e}")),
        }
    }
    Err(AppError::Io(format!(
        "全部行情主机请求失败: {}",
        failures.join("; ")
    )))
}

/// 同 [`request_json_from_hosts`] 的多主机切换，但返回**原始文本**（非 JSON 的
/// 数据文件通道用，如基金详情页 `.js`；issue #1062）。解析与可信度判定留给调用方
/// （文本层无法区分正常数据文件与被拦截 HTML 页，那是解析层的判据）。
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
    let mut failures: Vec<String> = Vec::new();
    for host in hosts {
        let url = format!("{host}{path}");
        match request_text_with_retry(client, &url, params, pacer, ctx, cfg, referer).await {
            Ok(resp) => return Ok(resp),
            Err(e) => failures.push(format!("{host}: {e}")),
        }
    }
    Err(AppError::Io(format!(
        "全部行情主机请求失败: {}",
        failures.join("; ")
    )))
}

/// 同 [`request_text_from_hosts`] 的多主机切换，但返回**原始字节**（非 UTF-8
/// 编码的通道用，如 GBK 报文；issue #1558）。复用同一套重试 / 多主机 / 限流冷却，
/// 解码与可信度判定留给调用方（字节层无法区分正常 GBK 报文与被拦截 HTML 页）。
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
    let mut failures: Vec<String> = Vec::new();
    for host in hosts {
        let url = format!("{host}{path}");
        // 纯字节通道的解析恒成功（解码与形状判定在调用方），因此不命中
        // `request_with_retry` 的「疑似被风控页」长冷却重试。
        let parsed = request_with_retry(client, &url, params, pacer, ctx, cfg, referer, &|bytes| {
            Ok(bytes.to_vec())
        })
        .await;
        match parsed {
            Ok(resp) => return Ok(resp),
            Err(e) => failures.push(format!("{host}: {e}")),
        }
    }
    Err(AppError::Io(format!(
        "全部行情主机请求失败: {}",
        failures.join("; ")
    )))
}

/// GBK 字节 → 文本：GBK 通道的解码原语，收口在本层单点（腾讯行情报价与新浪
/// 场外基金批量面共用，issue #1558 / #1564）。解码出错（截断 / 非法字节序列）
/// 即 fail-closed；合法但非 GBK 的内容（如被拦截页）留给调用方的形状判据处理。
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

/// 单主机请求重试 + 原样文本返回（非 JSON 数据文件通道，issue #1062）。
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

/// 单主机请求重试核心：串行限速 → 发送 → 传输错误短退避 / 429 长冷却 / 响应字节
/// 解析；解析失败按「疑似被风控拦截」长冷却重试，与既有 JSON 行为一致。纯文本通道
/// 的解析恒成功，据此复用同一套重试。异步形态下所有等待（限速、退避、冷却）等价
/// 迁移为异步睡眠，串行与重试语义不变（ADR-0125 决策 5/6）。
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
        // 部分行情接口要求带 Referer 头模拟站内跳来源，缺省被拦截：新浪批量面
        // 缺 Referer 返回 403（issue #1564）；已退役的东财 lsjz 亦同（issue #303）。
        let mut req = client.get(url).query(params).timeout(REQUEST_TIMEOUT);
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
            // 限速自适应（ADR-0121 决策 5）：限流响应即降速一档，冷却等待复用
            // 既有 `throttle_cooldown`（不新增等待机制）。
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
            return Err(AppError::Io("接口限流(429)，请稍后再试".into()));
        }

        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(ctx = %ctx, error = %e, "读取响应失败");
                sleep(cfg.throttle_cooldown).await;
                continue;
            }
        };
        match parse(&bytes) {
            Ok(parsed) => {
                // 成功即逐步回升（恢复后不一步跳回基线）。
                pacer.record_success();
                return Ok(parsed);
            }
            Err(e) => {
                let head = String::from_utf8_lossy(&bytes[..bytes.len().min(120)]);
                throttle_attempts += 1;
                // 疑似风控拦截页（非 JSON 响应）与 429 同待遇：降速一档再冷却。
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

/// 日 K 线单根样本：交易日（ISO 日期）与收盘价（真实价格值，非 f2 缩放值）。
/// 字段私有、构造经 [`KlineBar::new`]——通道束是壳层注入接缝的公开面，桩实现
/// 方需要能构造应答形状（issue #1375 后台补全注入接缝同需）。
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
