//! 行情同步域模型（#407 随域归位）：同步控制三类型 + 基金行情 DTO。
//!
//! 自全局模型目录迁入本域（#407 / #417 归属原则：类型归拥有它的域）：
//! - 同步控制三类型（进度事件载荷 / 中断结果 / 增量同步结果）仅被本域引擎与
//!   IPC 壳消费，不进 OpenAPI 契约面；
//! - 基金行情 DTO（[`FundDetail`] / [`FundNav`]）由本域基金访问层生产，投资域
//!   与 API 壳经域路径消费（编排接缝注入获取函数、BDD stub 构造跨 crate 使用）。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SyncProgress {
    pub current: usize,
    pub total: usize,
    pub market: String,
    pub done: bool,
    pub total_inserted: usize,
    pub total_updated: usize,
    pub error: Option<String>,
    /// 终态是否被中断（issue #104）：`done=true` 且 `cancelled=true` 表示同步被中断，
    /// `done=true` 且 `cancelled=false` 表示同步正常完成。`done=false` 时忽略此字段。
    pub cancelled: bool,
}

/// 全量同步中断命令的结果（issue #104）：`cancelled=true` 表示确实中断了一个正在进行的同步，
/// `false` 表示调用时无同步在跑（无副作用）。`message` 为可直接展示的中文提示。
#[derive(Debug, Clone, Serialize)]
pub struct CancelSyncResult {
    /// 是否确实中断了一个正在进行的全量同步。
    pub cancelled: bool,
    /// 结果提示文案（无同步时为「当前没有正在进行的同步」，否则「已请求中断同步」）。
    pub message: String,
}

/// 持仓价格增量同步结果（issue #103，#303 基金分区）：按类型分区刷价，
/// 不增删、不改标的字典。无持仓时返回明确提示而非报错。
#[derive(Debug, Clone, Serialize)]
pub struct SyncHoldingPricesResult {
    /// 处理成功的标的数：股票有效价 + 基金处理成功（含基金「已是最新、无新净值」）。
    pub synced: usize,
    /// 跳过数：债券等无行情来源持仓 + 名称充代码的基金行（无真实代码查不到净值）
    /// + 首刷查无净值的基金 + 市场未知（无法构造查询）+ 停牌/无效价/查询无果。
    pub skipped: usize,
    /// 结果提示文案（无持仓时为「无持仓标的可同步」，否则「已同步 N 只，跳过 M 只」），
    /// 供前端轻量消息直接展示。
    pub message: String,
    /// 实际写入价格的标的数（股票有效价 + 基金实际落库净值）：价格失效信号判定
    /// 依据（ADR-0031 零变化不广播，基金「已是最新」不算写入）。不进 IPC 线
    /// （前端无需感知，消息统计已含）。
    #[serde(skip)]
    pub written: usize,
}

/// 按代码即拉拉取到的基金详情（issue #301 / ADR-0038 决策 1）：名称与东财分类
/// 为透传展示信息（不落库），nav 缺省（新发基金尚未公布首期净值等）时仅建
/// 标的、不落现价（不广播价格失效信号）。
#[derive(Debug, Clone, PartialEq)]
pub struct FundDetail {
    pub code: String,
    pub name: String,
    /// 东财基金分类（如「混合型-灵活」），展示与 AI 确认识别用，不落库。
    pub fund_class: String,
    pub nav: Option<FundNav>,
}

/// 基金最新单位净值（真实价格值，元）与其净值日期（ISO 日期）。
#[derive(Debug, Clone, PartialEq)]
pub struct FundNav {
    pub nav: f64,
    pub nav_date: String,
}
