//! 行情 HTTP 网络层（issue #89）：东财 clist 接口请求、多主机切换、重试与限流冷却、
//! 响应解析。与数据库、进度事件无关，可独立测试（见 `tests.rs` 中本地 HTTP 服务用例）。

use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::error::{AppError, Result};

// 行情接口路径。东财 clist 接口同一数据结构分布在多个主机，按顺序尝试，失败自动切换下一个。
const API_PATH: &str = "/api/qt/clist/get";
// 批量报价接口路径：按 secid 一次携带多只跨市场代码查询最新价（增量同步用，issue #103）。
// 响应结构与 clist 一致（data.total / data.diff，条目 f12/f14/f2），复用同一套解析。
const ULIST_PATH: &str = "/api/qt/ulist.np/get";
// 每批最多携带的 secid 数（东财批量报价接口支持一次查多只，约 50 只/请求已足够小、避开限流）。
pub(super) const ULIST_BATCH_SIZE: usize = 50;
// 优先使用延迟行情主机池：push2 实时主机曾被东财对该出口 IP 触发风控（连接重置），
// push2delay 返回相同数据结构且对批量访问更稳定；延迟行情对全量标的同步足够。
const API_HOSTS: &[&str] = &[
    "https://push2delay.eastmoney.com",
    "https://12.push2delay.eastmoney.com",
    "https://21.push2delay.eastmoney.com",
    "https://60.push2delay.eastmoney.com",
    "https://90.push2delay.eastmoney.com",
    "https://push2.eastmoney.com",
];
/// 每页条数：同步编排按此分页遍历。
pub(super) const PAGE_SIZE: usize = 100;
// 东方财富公开行情接口限频约 60 次/分钟（1 次/秒），此处留更多余量并串行访问。
// 出口 IP 会被 onegate WAF 间歇性限流（返回 200 非 JSON 拦截页或 429），限流窗口约 2-4 分钟自动恢复。
const REQUEST_INTERVAL: Duration = Duration::from_millis(2000);
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
    fn production() -> Self {
        Self {
            max_retries: MAX_RETRIES,
            base_backoff: BASE_BACKOFF,
            max_throttle_retries: MAX_THROTTLE_RETRIES,
            throttle_cooldown: THROTTLE_COOLDOWN,
        }
    }
}

/// 串行限速器：保证相邻两次 HTTP 请求之间至少间隔 interval。
pub(super) struct Pacer {
    last: Option<Instant>,
    interval: Duration,
}

impl Pacer {
    pub(super) fn new(interval: Duration) -> Self {
        Self {
            last: None,
            interval,
        }
    }

    pub(super) fn wait(&mut self) {
        if let Some(last) = self.last {
            let elapsed = last.elapsed();
            if elapsed < self.interval {
                thread::sleep(self.interval - elapsed);
            }
        }
        self.last = Some(Instant::now());
    }
}

impl Default for Pacer {
    fn default() -> Self {
        Self::new(REQUEST_INTERVAL)
    }
}

/// 市场配置：`fs` 为东财接口的板块筛选参数，`currency` 为该市场标的的本币。
pub(super) struct MarketConfig {
    pub(super) code: &'static str,
    pub(super) fs: &'static str,
    pub(super) name: &'static str,
    pub(super) currency: &'static str,
}

pub(super) const MARKETS: &[MarketConfig] = &[
    MarketConfig {
        code: "sh",
        fs: "m:1+t:2,m:1+t:23",
        name: "沪市",
        currency: "CNY",
    },
    MarketConfig {
        code: "sz",
        fs: "m:0+t:6,m:0+t:80",
        name: "深市",
        currency: "CNY",
    },
    MarketConfig {
        code: "hk",
        fs: "m:128+t:3,m:128+t:4",
        name: "港股",
        currency: "HKD",
    },
];

/// 行情接口返回的单个股票条目（字段 f12=代码, f14=名称, f2=价格原始值）。
/// 注意 f2 的隐含小数位因市场而异：A 股 2 位（f2=951 表示 9.51），港股 3 位（f2=475200 表示 475.200），
/// 因此这里保留原始 f2，换算成分在 `f2_to_cents` 按市场处理。
/// get_total 请求只带 fields=f12，响应条目可能缺 f14/f2，因此名称与价格均可缺省。
#[derive(Debug, Deserialize)]
pub(super) struct StockItem {
    #[serde(rename = "f12")]
    pub(super) code: String,
    #[serde(rename = "f14", default)]
    pub(super) name: String,
    #[serde(rename = "f2", default, deserialize_with = "deserialize_f2")]
    pub(super) price: Option<f64>,
}

fn deserialize_f2<'de, D>(d: D) -> std::result::Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(d)?;
    let raw = match value {
        serde_json::Value::Number(n) => n.as_f64(),
        _ => None,
    };
    Ok(raw.filter(|&p| p > 0.0))
}

/// 将原始 f2 换算为整数分：A 股 f2=价格×100（×1 即得分），港股 f2=价格×1000（÷10 得分）。
pub(super) fn f2_to_cents(raw: f64, market_code: &str) -> i64 {
    if market_code == "hk" {
        (raw / 10.0).round() as i64
    } else {
        raw.round() as i64
    }
}

/// 行情列表接口整体响应。
#[derive(Debug, Deserialize)]
pub(super) struct ClistResponse {
    pub(super) data: ClistData,
}

/// ulist 批量报价响应：`data` 可能为 null（全部代码无效时东财返回 `rc=102` 且 `data:null`），
/// 此时应视为无行情条目而非错误，保证增量同步「停牌/无效价不中断同步」语义。
#[derive(Debug, Deserialize)]
pub(super) struct UlistResponse {
    pub(super) data: Option<ClistData>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ClistData {
    pub(super) total: Option<u64>,
    pub(super) diff: Option<DiffField>,
}

/// data.diff 东财既可能返回按序号 key 的对象，也可能返回数组。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum DiffField {
    Object(HashMap<String, StockItem>),
    Array(Vec<StockItem>),
}

impl DiffField {
    pub(super) fn into_items(self) -> Vec<StockItem> {
        let mut items: Vec<StockItem> = match self {
            DiffField::Object(map) => {
                let mut pairs: Vec<_> = map.into_iter().collect();
                pairs.sort_by_key(|(k, _)| k.parse::<usize>().unwrap_or(usize::MAX));
                pairs.into_iter().map(|(_, v)| v).collect()
            }
            DiffField::Array(items) => items,
        };
        items.retain(|s| !s.code.is_empty() && !s.name.is_empty());
        items
    }
}

/// 构建行情 HTTP 客户端（全量/增量同步共用，UA 保持一致）。
pub(super) fn build_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0")
        .build()
        .map_err(|e| AppError::Io(e.to_string()))
}

/// 市场代码 → 东财 secid 前缀（沪 1 / 深 0 / 港 116）。市场未知（unknown）无法查询，返回 None。
pub(super) fn secid_prefix(market: &str) -> Option<&'static str> {
    match market {
        "sh" => Some("1"),
        "sz" => Some("0"),
        "hk" => Some("116"),
        _ => None,
    }
}

/// 发送请求并解析 JSON，按序尝试多个主机，对传输错误做短退避、对限流拦截做长冷却重试。
fn request_json(
    client: &reqwest::blocking::Client,
    params: &[(&str, &str)],
    pacer: &mut Pacer,
    ctx: &str,
) -> Result<ClistResponse> {
    request_json_from_hosts(
        client,
        params,
        API_PATH,
        API_HOSTS,
        RetryConfig::production(),
        pacer,
        ctx,
    )
}

pub(super) fn request_json_from_hosts<T>(
    client: &reqwest::blocking::Client,
    params: &[(&str, &str)],
    path: &str,
    hosts: &[&str],
    cfg: RetryConfig,
    pacer: &mut Pacer,
    ctx: &str,
) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let mut failures: Vec<String> = Vec::new();
    for host in hosts {
        let url = format!("{host}{path}");
        match request_json_with_retry(client, &url, params, pacer, ctx, cfg) {
            Ok(resp) => return Ok(resp),
            Err(e) => failures.push(format!("{host}: {e}")),
        }
    }
    Err(AppError::Io(format!(
        "全部行情主机请求失败: {}",
        failures.join("; ")
    )))
}

pub(super) fn request_json_with_retry<T>(
    client: &reqwest::blocking::Client,
    url: &str,
    params: &[(&str, &str)],
    pacer: &mut Pacer,
    ctx: &str,
    cfg: RetryConfig,
) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let mut transport_attempts = 0u32;
    let mut throttle_attempts = 0u32;
    loop {
        pacer.wait();
        let resp = match client
            .get(url)
            .query(params)
            .timeout(REQUEST_TIMEOUT)
            .send()
        {
            Ok(r) => r,
            Err(e) => {
                transport_attempts += 1;
                if transport_attempts <= cfg.max_retries {
                    tracing::warn!(ctx = %ctx, attempt = transport_attempts, error = %e, "HTTP 请求失败，准备重试");
                    thread::sleep(cfg.base_backoff * (1u32 << (transport_attempts - 1)));
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
            if throttle_attempts <= cfg.max_throttle_retries {
                tracing::warn!(ctx = %ctx, attempt = throttle_attempts, "触发接口限流(429)，冷却后重试");
                thread::sleep(cfg.throttle_cooldown);
                continue;
            }
            return Err(AppError::Io("接口限流(429)，请稍后再试".into()));
        }

        let bytes = match resp.bytes() {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(ctx = %ctx, error = %e, "读取响应失败");
                thread::sleep(cfg.throttle_cooldown);
                continue;
            }
        };
        match serde_json::from_slice::<T>(&bytes) {
            Ok(json) => return Ok(json),
            Err(e) => {
                let head = String::from_utf8_lossy(&bytes[..bytes.len().min(120)]);
                throttle_attempts += 1;
                if throttle_attempts <= cfg.max_throttle_retries {
                    tracing::warn!(
                        ctx = %ctx, attempt = throttle_attempts, status = %status,
                        content_type = %content_type, content_encoding = %content_encoding,
                        body_head = %head, error = %e,
                        "响应解析失败（疑似被风控拦截），冷却后重试"
                    );
                    thread::sleep(cfg.throttle_cooldown);
                    continue;
                }
                tracing::error!(
                    ctx = %ctx, status = %status, content_type = %content_type,
                    content_encoding = %content_encoding, body_head = %head, error = %e,
                    "响应解析失败"
                );
                return Err(AppError::Parse(format!("JSON 解析失败: {e}")));
            }
        }
    }
}

pub(super) fn fetch_page(
    client: &reqwest::blocking::Client,
    pacer: &mut Pacer,
    market: &MarketConfig,
    page: usize,
) -> Result<Vec<StockItem>> {
    tracing::debug!(market = %market.name, page = %page, "获取股票数据页");
    let page_str = page.to_string();
    let size_str = PAGE_SIZE.to_string();
    let params = [
        ("fs", market.fs),
        ("pn", page_str.as_str()),
        ("pz", size_str.as_str()),
        ("fields", "f12,f14,f2"),
    ];
    let resp = request_json(
        client,
        &params,
        pacer,
        &format!("fetch_page:{}({})", market.name, page),
    )?;
    resp.data
        .diff
        .map(DiffField::into_items)
        .ok_or_else(|| AppError::Parse("响应中缺少 data.diff 字段".into()))
}

pub(super) fn get_total(
    client: &reqwest::blocking::Client,
    pacer: &mut Pacer,
    market: &MarketConfig,
) -> Result<usize> {
    let params = [
        ("fs", market.fs),
        ("pn", "1"),
        ("pz", "1"),
        ("fields", "f12"),
    ];
    let resp = request_json(
        client,
        &params,
        pacer,
        &format!("get_total:{}", market.name),
    )?;

    resp.data
        .total
        .map(|t| t as usize)
        .ok_or_else(|| AppError::Parse("响应中缺少 data.total 字段".into()))
}

/// 按 secid 批量查询最新价（跨市场一次携带多只，复用 clist 同一套主机池/重试/限流与解析）。
/// `secids` 为逗号分隔的东财 secid 串（形如 `1.600519,0.000001,116.00700`）。
/// 响应 `data` 为 null（全部代码无效）时返回空列表，不报错。
pub(super) fn fetch_ulist(
    client: &reqwest::blocking::Client,
    pacer: &mut Pacer,
    secids: &str,
) -> Result<Vec<StockItem>> {
    tracing::debug!(secids, "批量报价查询");
    let params = [("secids", secids), ("fields", "f12,f14,f2")];
    let resp: UlistResponse = request_json_from_hosts(
        client,
        &params,
        ULIST_PATH,
        API_HOSTS,
        RetryConfig::production(),
        pacer,
        "fetch_ulist",
    )?;
    Ok(resp
        .data
        .and_then(|d| d.diff)
        .map(DiffField::into_items)
        .unwrap_or_default())
}
