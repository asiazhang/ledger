//! 标的信息同步进度事件（issue #897 / ADR-0095）：同步编排过程中发出的
//! **带 payload 确定进度事件**，驱动前端确定性进度条。
//!
//! 与失效信号的划界（ADR-0031 / ADR-0044）：进度事件**不是**失效信号——
//! 带 payload（`{ done, total }`）、高频逐标的推进，语义锚「同步进行到哪」；
//! 价格失效信号（`ledger:prices-changed`）无 payload、粗粒度、只锚「数据变了」，
//! 二者同在 `ledger:*` 命名空间但互不替代，发射判定也不经 signals 映射单点
//!（映射单点只裁决失效信号，ADR-0044）。事件名常量与发射入口归本域收口
//!（与 `events` 模块的分工先例一致：不经失效信号映射的带 payload 事件只共用
//! 其 [`ledger_infra::events::post_emit_with`] 投递机制，本模块不进映射、不另起第二套）。
//!
//! 旧「标的全量同步」的进度事件（`sync-instruments:progress`）已随 ADR-0081
//! 决策 3 整体退役；本事件是现役增量同步（InstrumentInfoSync）上的重建，
//! 命名归位 `ledger:*` 命名空间。
//!
//! 价格历史后台补全（issue #1375）沿用同一 payload 形状、另起新事件名
//!（[`HISTORY_BACKFILL_PROGRESS`]，发射器接缝 [`BackfillProgressEmitter`]）
//! ——静默计数面与手动同步的进度条互不串台：两个事件各走各的发射器，
//! 后台推进绝不点亮前端的手动同步进度条，反之亦然。

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use ledger_infra::events::post_emit_with;

/// 标的信息同步进度事件名（issue #897 / ADR-0095；带 payload，与无 payload 的
/// 失效信号族同处 `ledger:*` 命名空间）。payload 见 [`SyncProgress`]。
pub const INSTRUMENT_SYNC_PROGRESS: &str = "ledger:instrument-sync-progress";

/// 价格历史后台补全进度事件名（issue #1375）：与手动同步进度事件同 payload
/// 形状（[`SyncProgress`]）、不同事件名——静默计数面的唯一事件来路。
pub const HISTORY_BACKFILL_PROGRESS: &str = "ledger:history-backfill-progress";

/// 汇率同步阶段文字进度事件名（issue #1762）：与手动同步进度事件同处
/// `ledger:*` 命名空间、不同事件名——汇率手动同步的阶段文字唯一事件来路。
/// 每日自动路径不发本事件（无 UI 消费），但在途互斥与手动路径共用同一门。
pub const FX_SYNC_PROGRESS: &str = "ledger:fx-sync-progress";

/// 汇率同步阶段闭集（issue #1762）：后端真实边界——下载与解析在取数腿内
/// 一体完成、不拆分。`fetching` = 正在读取（含下载与解析），`persisting` =
/// 正在写入。落库仍是单一事务整体回滚，不拆批、不报数——报数仅在读取阶段
/// 有价值（`days_parsed`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FxSyncStage {
    /// 正在读取汇率文件（含下载与解析）。
    Fetching,
    /// 正在写入（落库中）。
    Persisting,
}

/// 汇率同步阶段事件载荷（issue #1762）：阶段闭集 + 可选统计字段。读取阶段
/// 结束时携带「已解析天数」（`days_parsed`，单文件整包收完再解析、一次性上报）。
/// 事件契约留扩展位：将来升级实时字节 / 实时天数计数不换事件名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FxSyncProgress {
    /// 当前阶段（闭集）。
    pub stage: FxSyncStage,
    /// 已解析天数：读取完成时一次性上报，读取开始时为 `None`。
    pub days_parsed: Option<usize>,
}

impl FxSyncProgress {
    /// 读取开始：阶段进入 `fetching`，暂无计数。
    pub fn fetching() -> Self {
        Self {
            stage: FxSyncStage::Fetching,
            days_parsed: None,
        }
    }

    /// 写入开始：读取已完成，携带本次共解析天数。
    pub fn persisting(days_parsed: usize) -> Self {
        Self {
            stage: FxSyncStage::Persisting,
            days_parsed: Some(days_parsed),
        }
    }
}

/// 汇率阶段发射器接缝（issue #1762）：与 [`ProgressEmitter`] 同型的类型化载体，
/// 仅事件名与载荷不同（[`FX_SYNC_PROGRESS`] / [`FxSyncProgress`]）。唯一实现约定
/// 同前：**非阻塞**交接即返回；投递失败静默。独立 trait 而非复用 [`ProgressEmitter`]，
/// 因为 `AppHandle` 只能有一个同名 trait 实现：汇率阶段推进才不会点亮标的信息
/// 同步的确定进度条，反之亦然。
pub trait FxProgressEmitter: Send + Sync {
    /// 投递一次汇率阶段推进。实现必须非阻塞：交接即返回，不等送达。
    fn emit_fx_progress(&self, progress: FxSyncProgress);
}

/// 生产实现：与 [`ProgressEmitter for AppHandle`] 同一投递机制，事件名换本事件。
impl<R: Runtime> FxProgressEmitter for AppHandle<R> {
    fn emit_fx_progress(&self, progress: FxSyncProgress) {
        let handle = self.clone();
        post_emit_with(self, move || {
            let _ = handle.emit(FX_SYNC_PROGRESS, progress);
        });
    }
}
/// 进度事件载荷：`done` = 已完成的有通道标的数，`total` = 有通道标的总数
///（分母口径见 ADR-0095：行情分区逐标的计 1、有码基金逐只计 1，跳过行不计）。
/// `total` 随收集完成立即发出（`done = 0`）；此后每完成一个有通道标的推进一格。
///
/// 原基金逐只翻页的**页级明细**（`fund`，issue #1061 / ADR-0095 修订记录）随
/// 逐只净值通道换源新浪单只全历史面退役（issue #1571）：逐只回退与历史回填
/// 均一次请求整只历史，无翻页长等待，事件恒为 `{ done, total }` 形状。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncProgress {
    pub done: usize,
    pub total: usize,
}

impl SyncProgress {
    /// 标的级推进：`done` = 已完成的有通道标的数，`total` = 有通道标的总数。
    pub fn instrument(done: usize, total: usize) -> Self {
        Self { done, total }
    }
}

/// 进度发射器接缝（issue #897；与 `events::SignalEmitter` 同型的类型化载体）：
/// 「把一次进度推进投递出去」的机制抽象。唯一实现约定：**非阻塞**——实现只把
/// 投递动作交出去即返回，绝不等事件真正送达（进度事件高频，阻塞编排等同拖慢
/// 同步）；投递或发射失败静默忽略，不影响同步结果。生产实现为 [`AppHandle`]，
/// 测试注入记录型假发射器（接线证明与编排进度序列测试共用此接缝）。
pub trait ProgressEmitter: Send + Sync {
    /// 投递一次进度推进。实现必须非阻塞：交接即返回，不等送达。
    fn emit_progress(&self, progress: SyncProgress);
}

/// 生产实现：payload 事件经 [`post_emit_with`] 投递**主线程事件循环队尾**非阻塞
/// 执行（spec #364 / ADR-0054 同一投递机制——编排运行在写线程/阻塞线程池上，
/// 就地 emit 会走 `webviews_lock` 同步等待路径，有跨线程死锁前科）。
impl<R: Runtime> ProgressEmitter for AppHandle<R> {
    fn emit_progress(&self, progress: SyncProgress) {
        let handle = self.clone();
        post_emit_with(self, move || {
            let _ = handle.emit(INSTRUMENT_SYNC_PROGRESS, progress);
        });
    }
}

/// 后台补全进度发射器接缝（issue #1375）：与 [`ProgressEmitter`] 同型的类型化
/// 载体，仅事件名不同（[`HISTORY_BACKFILL_PROGRESS`]）——payload 同为
/// [`SyncProgress`]。唯一实现约定同前：**非阻塞**交接即返回；投递失败静默。
/// 独立 trait 而非复用 [`ProgressEmitter`]，因为 `AppHandle` 只能有一个同名
/// trait 实现：两个事件名必须各走各的接缝，后台推进才不会点亮手动同步进度条。
pub trait BackfillProgressEmitter: Send + Sync {
    /// 投递一次后台补全进度推进。实现必须非阻塞：交接即返回，不等送达。
    fn emit_backfill_progress(&self, progress: SyncProgress);
}

/// 生产实现：与 [`ProgressEmitter for AppHandle`] 同一投递机制，事件名换本事件。
impl<R: Runtime> BackfillProgressEmitter for AppHandle<R> {
    fn emit_backfill_progress(&self, progress: SyncProgress) {
        let handle = self.clone();
        post_emit_with(self, move || {
            let _ = handle.emit(HISTORY_BACKFILL_PROGRESS, progress);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_name_lives_in_ledger_namespace() {
        // 与失效信号族同一 ledger:* 命名空间（ADR-0095）；带 payload 的进度事件
        // 不冒充 `<domain>-changed` 失效信号形状。
        assert_eq!(INSTRUMENT_SYNC_PROGRESS, "ledger:instrument-sync-progress");
        assert!(INSTRUMENT_SYNC_PROGRESS.starts_with("ledger:"));
        assert!(!INSTRUMENT_SYNC_PROGRESS.ends_with("-changed"));
    }

    #[test]
    fn payload_serializes_as_done_total() {
        // 前端消费契约：标的级推进即 { done, total } 两字段数字对象（spec #897）。
        // 原基金页级明细字段（issue #1061）已随逐只通道换源退役（issue #1571），
        // 事件恒为两字段形状。
        let json = serde_json::to_value(SyncProgress {
            done: 37,
            total: 100,
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({ "done": 37, "total": 100 }));
    }

    #[test]
    fn fx_event_name_lives_in_ledger_namespace() {
        // 汇率阶段事件与标的信息进度事件同处 ledger:* 命名空间（issue #1762）；
        // 带 payload 的阶段事件不冒充 `<domain>-changed` 失效信号形状。
        assert_eq!(FX_SYNC_PROGRESS, "ledger:fx-sync-progress");
        assert!(FX_SYNC_PROGRESS.starts_with("ledger:"));
        assert!(!FX_SYNC_PROGRESS.ends_with("-changed"));
    }

    #[test]
    fn fx_payload_serializes_as_stage_with_optional_days() {
        // 前端消费契约（issue #1762）：阶段闭集 + 可选已解析天数——读取开始无计数、
        // 写入开始携带本次共解析天数。
        let fetching = serde_json::to_value(FxSyncProgress::fetching()).unwrap();
        assert_eq!(
            fetching,
            serde_json::json!({ "stage": "fetching", "days_parsed": null })
        );
        let persisting = serde_json::to_value(FxSyncProgress::persisting(21)).unwrap();
        assert_eq!(
            persisting,
            serde_json::json!({ "stage": "persisting", "days_parsed": 21 })
        );
    }
}
