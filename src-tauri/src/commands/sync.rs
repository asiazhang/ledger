//! 标的信息同步 IPC 命令壳（issue #89；#407 域目录化后压平为单文件纯壳；
//! issue #827 命令改名 `sync_holding_prices` → `sync_instrument_info`）。
//!
//! `sync_instrument_info` 写路径与信号经壳层统一写入口
//! [`crate::write_entry::write_entry`]（ADR-0073）：仪式内化单点，证据随闭包
//! 返回必达。标的全量同步命令/中断命令与进度事件已随 ADR-0081 决策 3 整体
//! 退役（issue #698）：股票字典修正归「按代码查询/创建带回权威名称」。

// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::{AppHandle, State};

use crate::db::DbState;
use crate::error::Result;
use crate::signals::{WriteEvidence, WriteOp};
use crate::sync::{ProgressEmitter, SyncInstrumentInfoResult, SyncProgress, do_incremental_sync};
use crate::write_entry::{Outcome, write_entry};

/// 生产进度接线（issue #897 / ADR-0095）：把编排的进度回调（`done, total`）接到
/// 进度事件发射器。独立成函数是壳层接线证明的锚点：测试注入记录型发射器驱动
/// 编排，钉住「命令壳把编排回调接到事件发射」；命令体一行调用本函数。
pub(crate) fn progress_to_emitter(emitter: &dyn ProgressEmitter) -> impl FnMut(usize, usize) + '_ {
    move |done, total| emitter.emit_progress(SyncProgress { done, total })
}

/// IPC 命令：同步标的信息（增量同步，issue #103 / #303；#827 覆盖面放开至
/// 库内全部标的并改名）。单次按通道分区刷价并随行刷新名称：股票/场内 ETF 走
/// 行情报价/K 线通道，场外基金走历史净值通道逐只按水位增量回填（ADR-0038 决策
/// 6）；有通道的行以数据源权威名称随行刷新（ADR-0036/0081 修订）；无通道行计入
/// 跳过；空库返回明确提示而非报错。异步执行（后台线程池），不阻塞主线程；返回
/// 结果统计（同步 N 只 / 跳过 M 只），前端据此轻量提示。成功且实际写入价格**或**
/// 名称（`any_written()`）时发价格失效信号（ADR-0031）：零变化（空库/全部跳过/
/// 基金无新净值且名称无变化）为库内零变化，不广播。
///
/// 同步全程发确定进度事件（issue #897 / ADR-0095）：编排的进度回调经
/// [`progress_to_emitter`] 接到 `ledger:instrument-sync-progress` 带 payload 事件
///（`{ done, total }`，经 [`ProgressEmitter`] 非阻塞投递主线程）；进度事件不是
/// 失效信号，不影响下方价格失效信号的判定与发射条件。
///
/// 「是否发」判定已于 #333 归一化进 signals 映射单点（`signals_for` +
/// [`WriteEvidence::PriceWritten`]，ADR-0044）：入口只把终态归一化为证据——
/// 到达保留落库的终态按 `result.any_written()`（价格或名称实际写入），失败无
/// 证据零信号（写失败早退不发）。
#[tauri::command]
pub async fn sync_instrument_info(
    db: State<'_, DbState>,
    app: AppHandle,
) -> Result<SyncInstrumentInfoResult> {
    let conn = db.conn.clone();
    // 进度发射器归进写闭包自有的一份句柄：`app` 同时被下方发射器参数借用，
    // 闭包（Send + 'static）捕获克隆件（issue #897）。
    let progress_app = app.clone();
    write_entry(
        "sync_instrument_info",
        conn,
        Some(&app),
        WriteOp::SyncInstrumentInfo,
        // 行情/汇率/历史/名称落库成功即置脏，提交点写时顺带到期检查（ADR-0032，
        // #246 审计补齐）；锁语义与先前整段持有一致（同步期间独占连接）。
        move |conn| {
            // 进度接线（issue #897）：编排进度回调 → 进度事件发射（非阻塞投递，
            // 发射失败静默，不影响同步结果）。
            let mut progress = progress_to_emitter(&progress_app);
            do_incremental_sync(conn, &mut progress).map(|result| {
                // 证据 = 价格或名称实际写入（issue #827）：名称刷新同样让标的列表
                // 失真，与价格写入同路计入「数据变了」。
                let evidence = WriteEvidence::PriceWritten(result.any_written());
                Outcome::Evidenced(result, evidence)
            })
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 记录型假发射器：接收到的进度推进按序攒入缓冲（发射器接缝的测试注入
    /// 先例：`test_utils::GatedEmitter` / `signals` 映射测试同纪律）。
    #[derive(Default)]
    struct RecordingEmitter(Mutex<Vec<SyncProgress>>);

    impl ProgressEmitter for RecordingEmitter {
        fn emit_progress(&self, progress: SyncProgress) {
            self.0.lock().unwrap().push(progress);
        }
    }

    /// 壳层接线证明（issue #897 / ADR-0095；先例：信号发射两层测试——映射层
    /// 钉「谁发什么」，本测试钉「命令壳把编排回调接到事件发射」的接线半边，
    /// 编排回调的触发时序归 sync 域进度序列测试）：`progress_to_emitter` 把
    /// 编排的 (done, total) 回调接到发射器，载荷形状与顺序保持不变。
    #[test]
    fn command_shell_wires_orchestration_progress_to_emitter() {
        let emitter = RecordingEmitter::default();
        let mut progress = progress_to_emitter(&emitter);

        progress(0, 100);
        progress(37, 100);
        progress(100, 100);

        assert_eq!(
            *emitter.0.lock().unwrap(),
            vec![
                SyncProgress {
                    done: 0,
                    total: 100
                },
                SyncProgress {
                    done: 37,
                    total: 100
                },
                SyncProgress {
                    done: 100,
                    total: 100
                },
            ],
            "编排回调逐次直达发射器，载荷即 done/total 两字段"
        );
    }
}
