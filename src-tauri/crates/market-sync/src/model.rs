//! 行情同步域模型（#407 随域归位）：标的信息同步结果类型。
//!
//! 自全局模型目录迁入本域（#407 / #417 归属原则：类型归拥有它的域）：
//! 同步结果类型仅被本域引擎与 IPC 壳消费，不进 OpenAPI 契约面。行情载荷虽有
//! 一个录入入口在本域，但归投资域引擎消费——#422 Q11 归属修正后迁入投资域，
//! ADR-0103 又收口为行情接入接缝的统一报价载荷（`investment::quote::Quote`），
//! 本域经域路径消费。全量同步的控制类型（进度事件载荷 / 中断结果）随 ADR-0081
//! 决策 3 退役删除（issue #698）。

use serde::Serialize;

/// 标的信息同步结果（issue #103，#303 基金分区，#827 覆盖面放开 + 名称随行刷新）：
/// 收集库内全部标的按通道能力分区刷价并随行刷新名称。空库返回明确提示而非报错。
#[derive(Debug, Clone, Serialize)]
pub struct SyncInstrumentInfoResult {
    /// 处理成功的标的数：行情分区有效价 + 基金处理成功（含基金「已是最新、无新净值」）。
    pub synced: usize,
    /// 跳过数：无通道行（债券等无行情来源、名称充代码的基金行（无真实代码查不到
    /// 净值）与市场未知（无法构造行情查询）的自建行）+ 停牌/无效价/查询无果
    /// + 首刷查无净值的基金 + 历史净值空响应（疑似被拦截/异常，issue #1059）。
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
    /// 本次同步是否降级走逐标的通道（ADR-0121 决策 3，issue #1374）：批量取数面
    /// 失败或处于跨同步停用期时为真。随 issue #1376 进 IPC 线：前端据此在界面
    /// 明示「已降级、本次较慢」，正常（批量面命中）路径不带该标注。
    pub bulk_degraded: bool,
    /// 批量取数面**覆盖缺口**的标的数（ADR-0121 决策 3，issue #1374）：批量面没
    /// 收录（新成立 / 已终止 / 清盘 / 部分货币基金）而逐条回退补齐的只数。缺口不是
    /// 失败——不进降级判定、不触发熔断，只在日志与统计上与失败分开。不进 IPC 线。
    #[serde(skip)]
    pub bulk_gaps: usize,
}

impl SyncInstrumentInfoResult {
    /// 是否发生任何实际写入（价格**或**名称）：价格失效信号的证据判定
    /// （ADR-0031 零变化不广播）。名称刷新单独落库后标的列表名称列同样失真，
    /// 与价格写入同路计入「数据变了」（issue #827）。
    pub fn any_written(&self) -> bool {
        self.written > 0 || self.renamed > 0
    }
}

/// 跨分段写入见证（issue #1277）：同步编排的「是否实际写过」累积器。
///
/// 增量同步是「抓取-落库交替」的长任务，分段形态逐段 autocommit：中途失败的
/// 运行前面分段可能已落库，而结果统计（[`SyncInstrumentInfoResult`]）随错误
/// 一同丢失——收尾裁决「实际写过即置脏即发信号」（成败同判，#1277 / ADR-0031）
/// 需要一份独立于成败的累积证据。见证器由调用方持有（`&mut` 传入编排），编排
/// 在每个实际写入点标记；失败时经壳层 [`SegmentedFailure`] 的证据位随错误必达
/// （成功时与结果统计的 [`SyncInstrumentInfoResult::any_written`] 同口径等价，
/// 测试钉住）。
///
/// 汇率 K 线落库不计入：与成功路径的零写入判定同口径（只有价格或名称写入
/// 才发价格失效信号，`fx_rate_history` 变化不在其列）。
///
/// [`SegmentedFailure`]: ledger_infra::shell_support::write_entry::SegmentedFailure
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteWitness {
    /// 是否发生过任何实际写入（幂等累积：见证器只回答「是否写过」）。
    written: bool,
}

impl WriteWitness {
    /// 记录一次实际写入（编排在每个落库成功点调用；幂等）。
    pub fn mark_written(&mut self) {
        self.written = true;
    }

    /// 是否发生过任何实际写入（与成功路径
    /// [`SyncInstrumentInfoResult::any_written`] 同口径：价格或名称）。
    pub fn any_written(&self) -> bool {
        self.written
    }
}
