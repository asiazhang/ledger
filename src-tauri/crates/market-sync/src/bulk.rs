//! 行情批量取数面（ADR-0121 / issue #1374）：名称全量字典 + 场外基金净值全市场
//! 批量面——回答「这次刷新用几次请求」，不回答价格从哪条通道写入（价格来源与
//! 来源标记不动，决策 2）。
//!
//! 不变量：两个面各整次同步最多一次请求；取数失败一律 fail-closed 回退逐标的
//! 通道并熔断本次同步，连续失败由跨同步记忆（[`BulkFetchCircuit`]）停用后半开；
//! 批量面未覆盖的标的按缺口逐条回退——缺口不是失败。陷阱：面报文是 `var x = …`
//! 形态的非 JSON 文本，被拦截形态必须报错、不得伪装成「零覆盖」（决策 3）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::Mutex as AsyncMutex;

use ledger_infra::error::{AppError, Result};

use super::http::{Pacer, RetryConfig, block_on, lock_pacer, request_text_from_hosts};

/// 全量名称字典：基金代码 → 数据源权威名称。
pub type FundNameDictionary = HashMap<String, String>;

/// 批量面的最新净值条目：净值日期 + 单位净值（真实价格值，元，口径同 lsjz 的 `DWJZ`）。
#[derive(Debug, Clone, PartialEq)]
pub struct BulkNavPoint {
    pub date: String,
    pub nav: f64,
}

impl BulkNavPoint {
    /// 该条目是否**不晚于**给定水位（现价缓存的净值日期）：不晚于即为「没有新净值」。
    /// 水位缺失时恒为 false——无从判定「没有新净值」，是否发请求由逐只通道说了算。
    /// 日期是 ISO 形态，字符串序即日期序（与逐只通道的水位判据同口径）。
    pub fn is_not_newer_than(&self, watermark: Option<&str>) -> bool {
        watermark.is_some_and(|watermark| self.date.as_str() <= watermark)
    }
}

/// 全市场最新净值表：基金代码 → 最新单位净值与净值日期。
pub type FundNavTable = HashMap<String, BulkNavPoint>;

/// 批量面取回的数据都是「代码 → 值」的映射：命中日志据此统一记录覆盖规模。
pub(super) trait BulkCoverage {
    fn covered(&self) -> usize;
}

impl<K, V> BulkCoverage for HashMap<K, V> {
    fn covered(&self) -> usize {
        self.len()
    }
}

/// 名称全量字典抓取通道闭包形态（整次同步一次请求）。
pub type FetchFundNameDictionary = Box<dyn FnMut() -> Result<FundNameDictionary> + Send>;
/// 场外基金净值批量面抓取通道闭包形态（整次同步一次请求）。
pub type FetchFundNavTable = Box<dyn FnMut() -> Result<FundNavTable> + Send>;

/// 跨同步记忆阈值：连续这么多次同步的批量取数失败后停用批量面（ADR-0121 决策 3）。
pub const BULK_FAILURE_THRESHOLD: u32 = 3;
/// 停用期限：到期后先半开试一次（失败即重新计时，成功即解除）。
pub const BULK_DISABLE_PERIOD: Duration = Duration::from_secs(1800);

/// 批量取数面的跨同步记忆（ADR-0121 决策 3「跨同步轻微记忆」）：连续失败达
/// [`BULK_FAILURE_THRESHOLD`] 后停用批量面一个 [`BULK_DISABLE_PERIOD`]，避免在
/// 数据源风控窗口内每轮同步都去撞一次；停用到期放行**一次**半开试探——试探成功
/// 解除停用，失败则重新计时。状态是纯内存的进程内状态：进程重启即回正常态，
/// 不落库、不新增持久状态。
///
/// 时刻由调用方传入（[`Self::should_attempt`] 等），使状态机可被确定性驱动
/// （生产传 `Instant::now()`，测试传任意时刻）。
#[derive(Debug)]
pub struct BulkFetchCircuit {
    consecutive_failures: u32,
    /// 停用截止时刻（None = 未停用）。
    disabled_until: Option<Instant>,
}

impl BulkFetchCircuit {
    pub fn new() -> Self {
        Self {
            consecutive_failures: 0,
            disabled_until: None,
        }
    }

    /// 本次同步是否尝试批量取数面：未停用 → 是；停用期内 → 否；停用到期 → 放行
    /// 一次半开试探并把窗口推到下一期（试探结果未回来之前再次询问仍为「不尝试」，
    /// 风控窗口内不会每轮都撞）。
    pub fn should_attempt(&mut self, now: Instant) -> bool {
        match self.disabled_until {
            None => true,
            Some(until) if now < until => false,
            Some(_) => {
                self.disabled_until = Some(now + BULK_DISABLE_PERIOD);
                true
            }
        }
    }

    /// 本次同步的批量取数面成功：清零连续失败、解除停用。
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.disabled_until = None;
    }

    /// 本次同步的批量取数面失败一次：连续失败达阈值即停用一个期限（半开试探失败
    /// 同样走这里——计数已满时重新计时）。
    pub fn record_failure(&mut self, now: Instant) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= BULK_FAILURE_THRESHOLD {
            self.disabled_until = Some(now + BULK_DISABLE_PERIOD);
        }
    }

    /// 是否处于停用期（观测与测试用）。
    pub fn is_disabled(&self, now: Instant) -> bool {
        matches!(self.disabled_until, Some(until) if now < until)
    }

    /// 连续失败次数（观测与测试用）。
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }
}

impl Default for BulkFetchCircuit {
    fn default() -> Self {
        Self::new()
    }
}

/// 批量取数面的跨同步记忆句柄（进程内唯一一份）：通道束每次同步重建，记忆必须
/// 活在束之外，故生产用进程级单例、测试注入自建实例（互不串扰）。
pub fn shared_circuit() -> Arc<Mutex<BulkFetchCircuit>> {
    static SHARED: OnceLock<Arc<Mutex<BulkFetchCircuit>>> = OnceLock::new();
    SHARED
        .get_or_init(|| Arc::new(Mutex::new(BulkFetchCircuit::new())))
        .clone()
}

/// 两个批量取数面 + 跨同步记忆的打包束（与 [`super::channels::SyncFetchChannels`]
/// 同款换装形态：生产接 HTTP 层，测试注入桩）。
pub struct BulkFetchSurfaces {
    /// 名称全量字典（一次请求覆盖全市场基金代码与权威名称）。
    pub names: FetchFundNameDictionary,
    /// 场外基金净值批量面（一次请求覆盖全市场基金的最新单位净值与净值日期）。
    pub nav: FetchFundNavTable,
    /// 跨同步记忆（ADR-0121 决策 3）。
    pub circuit: Arc<Mutex<BulkFetchCircuit>>,
}

impl BulkFetchSurfaces {
    /// 生产构造：两面接 HTTP 层（复用主机池 / 重试 / 自适应限速 pacer），跨同步
    /// 记忆取进程级单例（每次同步新建通道束不保留状态）。
    pub(super) fn production(client: &reqwest::Client, pacer: Arc<AsyncMutex<Pacer>>) -> Self {
        Self {
            names: {
                let client = client.clone();
                let pacer = pacer.clone();
                Box::new(move || {
                    block_on(async {
                        let mut pacer = lock_pacer(&pacer).await;
                        fetch_fund_name_dictionary(&client, &mut pacer).await
                    })
                })
            },
            nav: {
                let client = client.clone();
                Box::new(move || {
                    block_on(async {
                        let mut pacer = lock_pacer(&pacer).await;
                        fetch_fund_nav_table(&client, &mut pacer).await
                    })
                })
            },
            circuit: shared_circuit(),
        }
    }

    /// 无批量取数面（逐标的通道直通）的**最小形状**：两面恒定报「零覆盖」，
    /// 所有标的按缺口走逐标的通道——与批量面被数据源整市场漏掉等价。测试注入用，
    /// 也是「取数面整体不可用」这一降级形态的对照物（缺口 ≠ 失败：不触发熔断）。
    /// 记忆句柄随构造独立发放，不共享生产单例（测试之间零串扰）。
    pub fn absent() -> Self {
        Self {
            names: Box::new(|| Ok(FundNameDictionary::new())),
            nav: Box::new(|| Ok(FundNavTable::new())),
            circuit: Arc::new(Mutex::new(BulkFetchCircuit::new())),
        }
    }
}

// 名称全量字典：东财静态数据文件（`var r = [["000001","HXCZHH","华夏成长混合",…], …]`），
// 单主机（无公开镜像池），复用行情层的重试与限流泛型层。
const FUND_NAME_DICTIONARY_HOSTS: &[&str] = &["https://fund.eastmoney.com"];
const FUND_NAME_DICTIONARY_PATH: &str = "/js/fundcode_search.js";

// 场外基金净值排行批量面：单主机（无公开镜像池）。`pn` 拉满即一次请求覆盖全市场
// （实测 pn=30000 → 20,360 只）；缺 Referer 会被接口以「无访问权限」拦截。
const FUND_NAV_RANKING_HOSTS: &[&str] = &["https://fund.eastmoney.com"];
const FUND_NAV_RANKING_PATH: &str = "/data/rankhandler.aspx";
const FUND_NAV_RANKING_REFERER: &str = "https://fund.eastmoney.com/data/fundranking.html";
/// 单页条数：实测服务端一次可给全市场（`pn=30000` → 20,360 只），日常路径不依赖分页。
const FUND_NAV_RANKING_PAGE_SIZE: &str = "30000";

/// 拉取名称全量字典（一次请求覆盖全市场基金代码与权威名称）。报文被拦截（风控
/// HTML 页）或数据数组不可信时返回 `Err`——调用方 fail-closed 回退逐只名称通道，
/// 不把不可信结果当「查无此码」。
pub(super) async fn fetch_fund_name_dictionary(
    client: &reqwest::Client,
    pacer: &mut Pacer,
) -> Result<FundNameDictionary> {
    fetch_fund_name_dictionary_from(client, pacer, FUND_NAME_DICTIONARY_HOSTS).await
}

/// 同 [`fetch_fund_name_dictionary`]，主机池可注入（本地 HTTP 服务测试请求形态与
/// 被拦截响应处置，先例：`fund_nav::fetch_nav_full_series_from`）。
pub(super) async fn fetch_fund_name_dictionary_from(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
) -> Result<FundNameDictionary> {
    tracing::debug!("基金名称全量字典查询");
    let body = request_text_from_hosts(
        client,
        &[],
        FUND_NAME_DICTIONARY_PATH,
        hosts,
        RetryConfig::production(),
        pacer,
        "fetch_fund_name_dictionary",
        None,
    )
    .await?;
    parse_fund_name_dictionary(&body).ok_or_else(|| {
        // 文本通道的解析恒成功，疑似风控页在 HTTP 层看不见——降速信号由做可信度
        // 判定的这一层补上（ADR-0121 决策 5）。
        pacer.record_throttled();
        AppError::Parse("基金名称全量字典缺少可信的数据数组（疑似被风控拦截）".into())
    })
}

/// 拉取场外基金净值批量面（一次请求覆盖全市场基金的最新单位净值与净值日期）。
/// 报文缺 `datas` 数组（被拦截 / `ErrCode=-999` 无权限）时返回 `Err`——调用方
/// fail-closed 回退逐只净值通道。
pub(super) async fn fetch_fund_nav_table(
    client: &reqwest::Client,
    pacer: &mut Pacer,
) -> Result<FundNavTable> {
    fetch_fund_nav_table_from(client, pacer, FUND_NAV_RANKING_HOSTS).await
}

/// 同 [`fetch_fund_nav_table`]，主机池可注入（本地 HTTP 服务测试请求参数 /
/// Referer 传播与被拦截响应处置）。
pub(super) async fn fetch_fund_nav_table_from(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
) -> Result<FundNavTable> {
    tracing::debug!("场外基金净值批量面查询");
    let params = [
        ("op", "ph"),
        ("dt", "kf"),
        ("ft", "all"),
        ("rs", ""),
        ("gs", "0"),
        ("sc", "1nzf"),
        ("st", "desc"),
        ("sd", ""),
        ("ed", ""),
        ("qdii", ""),
        ("tabSubtype", ",,,,,"),
        ("pi", "1"),
        ("pn", FUND_NAV_RANKING_PAGE_SIZE),
        ("dx", "1"),
    ];
    let body = request_text_from_hosts(
        client,
        &params,
        FUND_NAV_RANKING_PATH,
        hosts,
        RetryConfig::production(),
        pacer,
        "fetch_fund_nav_table",
        Some(FUND_NAV_RANKING_REFERER),
    )
    .await?;
    parse_fund_nav_table(&body).ok_or_else(|| {
        pacer.record_throttled();
        AppError::Parse("场外基金净值批量面缺少可信的数据数组（疑似被风控拦截）".into())
    })
}

/// 解析名称全量字典：`var r = [["000001","HXCZHH","华夏成长混合","混合型-灵活","…"], …]`，
/// 取每行的 `[0] 代码` 与 `[2] 名称`。零覆盖是**可信空结果**（字典为空即全部按缺口
/// 走逐只通道）；数据数组缺失或不合法（风控 HTML 页、变量改名）返回 None，调用方
/// fail-closed 回退逐只通道。行内异常（缺代码/缺名称）逐行跳过。
pub(super) fn parse_fund_name_dictionary(body: &str) -> Option<FundNameDictionary> {
    let array = super::js::declared_array(body, "var r")?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(array).ok()?;
    let mut dictionary = FundNameDictionary::new();
    for row in rows {
        let Some(cells) = row.as_array() else {
            continue;
        };
        let code = cells.first().and_then(|v| v.as_str()).map(str::trim);
        let name = cells.get(2).and_then(|v| v.as_str()).map(str::trim);
        let (Some(code), Some(name)) = (code, name) else {
            continue;
        };
        if code.is_empty() || name.is_empty() {
            continue;
        }
        dictionary.insert(code.to_string(), name.to_string());
    }
    Some(dictionary)
}

/// 解析场外基金净值批量面：`var rankData = {datas:["代码,简称,拼音,净值日期,单位净值,
/// 累计净值,…", …], …}`，取每行的 `[0] 代码`、`[3] 净值日期`、`[4] 单位净值`
/// （净值即价格，ADR-0038 决策 3）。列形态不符 / 净值非正的行逐行跳过（该只按缺口
/// 走逐只通道，等价于「排行面没收录它」）；数据数组缺失或不合法返回 None，调用方
/// fail-closed 回退逐只通道。
pub(super) fn parse_fund_nav_table(body: &str) -> Option<FundNavTable> {
    let array = super::js::declared_array(body, "datas")?;
    let rows: Vec<String> = serde_json::from_str(array).ok()?;
    let mut table = FundNavTable::new();
    for row in rows {
        let cells: Vec<&str> = row.split(',').collect();
        let (Some(code), Some(date), Some(nav)) = (cells.first(), cells.get(3), cells.get(4))
        else {
            continue;
        };
        let (code, date) = (code.trim(), date.trim());
        let Ok(nav) = nav.trim().parse::<f64>() else {
            continue;
        };
        if code.is_empty() || date.is_empty() || nav <= 0.0 {
            continue;
        }
        table.insert(
            code.to_string(),
            BulkNavPoint {
                date: date.to_string(),
                nav,
            },
        );
    }
    Some(table)
}
