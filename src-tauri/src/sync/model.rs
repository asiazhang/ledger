//! 行情同步域模型（#407 随域归位）：标的信息同步结果类型。
//!
//! 自全局模型目录迁入本域（#407 / #417 归属原则：类型归拥有它的域）：
//! 同步结果类型仅被本域引擎与 IPC 壳消费，不进 OpenAPI 契约面。基金行情
//! DTO（FundDetail / FundNav）虽有一个录入入口在本域，但被投资域引擎自身消费，
//! #422 Q11 归属修正后迁入投资域集中模型（`investment::model`），本域经域路径
//! 消费。全量同步的控制类型（进度事件载荷 / 中断结果）随 ADR-0081 决策 3
//! 退役删除（issue #698）。

use serde::Serialize;

/// 标的信息同步结果（issue #103，#303 基金分区，#827 覆盖面放开 + 名称随行刷新）：
/// 收集库内全部标的按通道能力分区刷价并随行刷新名称。空库返回明确提示而非报错。
#[derive(Debug, Clone, Serialize)]
pub struct SyncInstrumentInfoResult {
    /// 处理成功的标的数：行情分区有效价 + 基金处理成功（含基金「已是最新、无新净值」）。
    pub synced: usize,
    /// 跳过数：无通道行（债券等无行情来源、名称充代码的基金行（无真实代码查不到
    /// 净值）与市场未知（无法构造行情查询）的自建行）+ 停牌/无效价/查询无果
    /// + 首刷查无净值的基金。
    pub skipped: usize,
    /// 结果提示文案（空库时为「暂无标的可同步」，否则「已同步 N 只，跳过 M 只」），
    /// 供前端轻量消息直接展示。
    pub message: String,
    /// 实际写入价格的标的数（股票有效价 + 基金实际落库净值；基金「已是最新」不算）。
    /// 不进 IPC 线（前端无需感知，消息统计已含）。
    #[serde(skip)]
    pub written: usize,
    /// 名称随行刷新的标的数（issue #827）：名称被数据源权威名称覆盖的行数，
    /// 与 [`Self::written`] 一同参与零写入判定。不进 IPC 线。
    #[serde(skip)]
    pub renamed: usize,
}

impl SyncInstrumentInfoResult {
    /// 是否发生任何实际写入（价格**或**名称）：价格失效信号的证据判定
    /// （ADR-0031 零变化不广播）。名称刷新单独落库后标的列表名称列同样失真，
    /// 与价格写入同路计入「数据变了」（issue #827）。
    pub fn any_written(&self) -> bool {
        self.written > 0 || self.renamed > 0
    }
}
