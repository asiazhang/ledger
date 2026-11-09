//! 行情批量取数面（ADR-0121 / issue #1374）：场外基金批量面——一次请求携带本次
//! 同步现场的全部基金代码，报出数据源权威名称与最新单位净值（ADR-0130 决策 2
//! 换源为新浪 `f_` 面，issue #1565）；回答「这次刷新用几次请求」，不决定价格从哪
//! 条通道写入（来源标记按实际取数源取值的口径见 ADR-0130 决策 7）。
//!
//! 不变量：本面每次同步最多一次逻辑请求（批量承载量由取数层按实测请求行上限
//! 自行分批）；取数失败一律 fail-closed 回退逐标的通道并记入跨同步记忆，连续
//! 失败由 [`BulkFetchCircuit`] 停用后半开；面未返回的标的按缺口逐条回退——缺口
//! 不是失败。陷阱：面报文是 `var x = …` 形态的非 JSON 文本，被拦截形态必须报错、
//! 不得伪装成「零覆盖」（决策 3）。
//!
//! 名称与净值同面返回：`f_` 面按代码查询、逐行携带名称与净值位，因此名称字典
//! 与净值表来自同一次响应。**货基错位行只在名称字典、不在净值表**——万份收益
//! 在单位净值位（ADR-0130 决策 6），取数层判形为 `MoneyYield` 不产出价格点；
//! 消费方按「在名称字典、不在净值表」识别它，落逐只臂经官方披露判定门收尾。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::Mutex as AsyncMutex;

use super::channels::FetchFuture;
use super::http::{Pacer, lock_pacer};
use super::sina_fund::{fetch_sina_fund_nav_rows, fund_batch_from_rows};

/// 名称字典：基金代码 → 数据源权威名称（本次批量面覆盖的行，含货基）。
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

/// 最新净值表：基金代码 → 最新单位净值与净值日期（只收录产出价格点的行）。
pub type FundNavTable = HashMap<String, BulkNavPoint>;

/// 场外基金批量面载荷（名称 + 最新净值同面返回，issue #1565）：名称字典覆盖面内
/// 全部行（含货基），净值表只收录产出价格点的普通行。**`names` 是覆盖面的判据**
///（有名称即有该码），`nav` 是「面是否给出可采信的价格点」的判据——某码在
/// `names` 而不在 `nav` 即货基错位行（万份收益在单位净值位，ADR-0130 决策 6），
/// 消费方按缺口之外的「已收录、无价格点」语义落逐只臂。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FundBatch {
    pub names: FundNameDictionary,
    pub nav: FundNavTable,
}

impl FundBatch {
    /// 该码是否被本面收录（有名称即收录，与净值点无关）。
    pub fn covers(&self, code: &str) -> bool {
        self.names.contains_key(code)
    }

    /// 面给出的数据源权威名称（未收录为 None）。
    pub fn name_of(&self, code: &str) -> Option<&str> {
        self.names.get(code).map(String::as_str)
    }

    /// 面给出的最新单位净值点（未收录或货基错位行为 None）。
    pub fn nav_of(&self, code: &str) -> Option<&BulkNavPoint> {
        self.nav.get(code)
    }
}

/// 场外基金批量面抓取通道闭包形态（整次同步一次逻辑请求）：入参是本次同步现场
/// 的基金代码（新浪 `f_` 面按代码查询，名称与净值随行返回），网络等待以 `await`
/// 表达（ADR-0125 决策 5 / issue #1412），闭包返回装箱 future。
pub type FetchFundBatch = Box<dyn FnMut(&[String]) -> FetchFuture<FundBatch> + Send>;

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

/// 场外基金批量面 + 跨同步记忆的打包束（与 [`super::channels::SyncFetchChannels`]
/// 同款换装形态：生产接 HTTP 层，测试注入桩）。
///
/// 取数面成员随源替换（ADR-0121 修订）：新浪 `f_` 面把名称与最新净值放在同一
/// 次响应里（按代码查询），因此原来的「名称全量字典 + 排行批量面」两个成员合并
/// 为一个面，`degraded`/缺口/跨同步记忆语义不变（单面下「一面失败不再试其余面」
/// 自然消解）。
pub struct BulkFetchSurfaces {
    /// 场外基金批量面（名称 + 最新单位净值同面返回，issue #1565）。
    pub funds: FetchFundBatch,
    /// 跨同步记忆（ADR-0121 决策 3）。
    pub circuit: Arc<Mutex<BulkFetchCircuit>>,
}

impl BulkFetchSurfaces {
    /// 生产构造：本面接 HTTP 层（复用主机池 / 重试 / 自适应限速 pacer），跨同步
    /// 记忆取进程级单例（每次同步新建通道束不保留状态）。`hosts` 是新浪 `f_`
    /// 批量面主机（等价位置参数可互换编译，故由调用方具名传入）。
    pub(super) fn production(
        client: &reqwest::Client,
        pacer: Arc<AsyncMutex<Pacer>>,
        hosts: Vec<String>,
    ) -> Self {
        Self {
            funds: {
                let client = client.clone();
                let pacer = pacer.clone();
                Box::new(move |codes: &[String]| {
                    let codes = codes.to_vec();
                    let client = client.clone();
                    let pacer = pacer.clone();
                    let hosts = hosts.clone();
                    Box::pin(async move {
                        let mut pacer = lock_pacer(&pacer).await;
                        let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
                        // 批量承载量由取数层按实测请求行上限自行分批（issue #1564）。
                        let rows =
                            fetch_sina_fund_nav_rows(&client, &mut pacer, &hosts, &codes).await?;
                        Ok(fund_batch_from_rows(rows))
                    })
                })
            },
            circuit: shared_circuit(),
        }
    }

    /// 无批量取数面（逐标的通道直通）的**最小形状**：本面恒定报「零覆盖」，
    /// 所有标的按缺口走逐标的通道——与批量面被数据源整市场漏掉等价。测试注入用，
    /// 也是「取数面整体不可用」这一降级形态的对照物（缺口 ≠ 失败：不触发熔断）。
    /// 记忆句柄随构造独立发放，不共享生产单例（测试之间零串扰）。
    pub fn absent() -> Self {
        Self {
            funds: Box::new(|_: &[String]| Box::pin(async { Ok(FundBatch::default()) })),
            circuit: Arc::new(Mutex::new(BulkFetchCircuit::new())),
        }
    }
}
