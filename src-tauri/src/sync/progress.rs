//! 标的信息同步进度事件（issue #897 / ADR-0095）：同步编排过程中发出的
//! **带 payload 确定进度事件**，驱动前端确定性进度条。
//!
//! 与失效信号的划界（ADR-0031 / ADR-0044）：进度事件**不是**失效信号——
//! 带 payload（`{ done, total }`）、高频逐标的推进，语义锚「同步进行到哪」；
//! 价格失效信号（`ledger:prices-changed`）无 payload、粗粒度、只锚「数据变了」，
//! 二者同在 `ledger:*` 命名空间但互不替代，发射判定也不经 signals 映射单点
//!（映射单点只裁决失效信号，ADR-0044）。事件名常量与发射入口归本域收口
//!（与 `events` 模块的分工先例一致：不经失效信号映射的带 payload 事件只共用
//! 其 [`crate::events::post_emit_with`] 投递机制，本模块不进映射、不另起第二套）。
//!
//! 旧「标的全量同步」的进度事件（`sync-instruments:progress`）已随 ADR-0081
//! 决策 3 整体退役；本事件是现役增量同步（InstrumentInfoSync）上的重建，
//! 命名归位 `ledger:*` 命名空间。

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::events::post_emit_with;

/// 标的信息同步进度事件名（issue #897 / ADR-0095；带 payload，与无 payload 的
/// 失效信号族同处 `ledger:*` 命名空间）。payload 见 [`SyncProgress`]。
pub const INSTRUMENT_SYNC_PROGRESS: &str = "ledger:instrument-sync-progress";

/// 场外基金深回填的**页级明细**（issue #1061）：正在按页拉取的基金代码与
/// 「已完成页 / 总页数」。只在真正翻页（`pages > 1`）时随进度事件带出——
/// 单页的增量常态不产生该明细，既有事件形状与频率不变。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FundNavProgress {
    /// 正在回填的基金代码（即标的 symbol）。
    pub code: String,
    /// 已抓取并合入的最后一页（1 起）。
    pub page: u64,
    /// 本轮窗口的总页数（服务端硬上限 20 条/页，与触顶上限取小后的实际页数）。
    pub pages: u64,
}

/// 进度事件载荷：`done` = 已完成的有通道标的数，`total` = 有通道标的总数
///（分母口径见 ADR-0095：行情分区逐标的计 1、有码基金逐只计 1，跳过行不计）。
/// `total` 随收集完成立即发出（`done = 0`）；此后每完成一个有通道标的推进一格。
///
/// `fund`（issue #1061 / ADR-0095 修订记录）：正在深回填的基金页级明细，只在
/// 本页抓取**返回之后**发出——抓取内部的退避/重试等待不产生推进；`done` /
/// `total` 的标的级口径不因页推进而改变。缺省时序列化省略该字段，标的级事件
/// 仍是既有的 `{ done, total }` 形状。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncProgress {
    pub done: usize,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fund: Option<FundNavProgress>,
}

impl SyncProgress {
    /// 标的级推进（不带页级明细）：`done` = 已完成的有通道标的数，`total` =
    /// 有通道标的总数。基金深回填期间另带 [`FundNavProgress`]，不在此构造。
    pub fn instrument(done: usize, total: usize) -> Self {
        Self {
            done,
            total,
            fund: None,
        }
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
impl ProgressEmitter for AppHandle {
    fn emit_progress(&self, progress: SyncProgress) {
        let handle = self.clone();
        post_emit_with(self, move || {
            let _ = handle.emit(INSTRUMENT_SYNC_PROGRESS, progress);
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
        // 前端消费契约：标的级推进即 { done, total } 两字段数字对象（spec #897）；
        // 基金页级明细缺省时不序列化（issue #1061），形状对旧消费方零破坏。
        let json = serde_json::to_value(SyncProgress {
            done: 37,
            total: 100,
            fund: None,
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({ "done": 37, "total": 100 }));
    }

    #[test]
    fn payload_carries_fund_page_detail_when_present() {
        // issue #1061：深回填期间带出基金代码与页级进度；done/total 仍是标的级
        // 口径（此处单只基金 = 0/1），页推进不改这两个数字。
        let json = serde_json::to_value(SyncProgress {
            done: 0,
            total: 1,
            fund: Some(FundNavProgress {
                code: "110022".into(),
                page: 3,
                pages: 25,
            }),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "done": 0,
                "total": 1,
                "fund": { "code": "110022", "page": 3, "pages": 25 },
            })
        );
    }
}
