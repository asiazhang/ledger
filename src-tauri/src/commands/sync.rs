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

use rusqlite::Connection;
use tauri::{AppHandle, State};

use crate::db::DbState;
use crate::error::Result;
use crate::signals::{WriteEvidence, WriteOp};
use crate::sync::{
    ProgressEmitter, ScopedSession, SyncInstrumentInfoResult, SyncProgress, do_incremental_sync,
};
use crate::write_entry::{Outcome, write_entry};

/// 生产会话接线（issue #1275）：把统一写入口闭包**已持有**的连接包成作用域
/// 会话交给编排（[`ScopedSession`] 壳层实现）。本票为预重构，实现是直通：编排
/// 每次取连接拿到的都是这把已持有的连接，锁跨度与先前「整段持有」逐字节一致
/// （行为零变化）；作用域的意义在形状——编排路径在类型上取不到连接，网络 I/O
/// 只能发生在会话之外（ADR-0069 决策 4 的结构化收口）。写入口改分段取锁后
/// （父 spec #1274 实现决策 4），此处换装锁内短暂获取的会话实现，编排侧零改动。
pub(crate) struct WriteEntrySession<'a> {
    conn: &'a Connection,
}

impl ScopedSession for WriteEntrySession<'_> {
    fn with_connection<R, F>(&self, use_connection: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R>,
    {
        use_connection(self.conn)
    }
}

/// 生产进度接线（issue #897 / ADR-0095）：把编排的进度回调（标的级 `done`/
/// `total`，基金深回填时带页级明细，issue #1061）接到进度事件发射器。独立成
/// 函数是壳层接线证明的锚点：测试注入记录型发射器驱动编排，钉住「命令壳把编排
/// 回调接到事件发射」；命令体一行调用本函数。
pub(crate) fn progress_to_emitter(emitter: &dyn ProgressEmitter) -> impl FnMut(SyncProgress) + '_ {
    move |progress| emitter.emit_progress(progress)
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
///（标的级 `{ done, total }`，基金深回填期间另带页级明细 `fund`，issue #1061；
/// 经 [`ProgressEmitter`] 非阻塞投递主线程）；进度事件不是失效信号，不影响下方
/// 价格失效信号的判定与发射条件。
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
            // 发射失败静默，不影响同步结果）。会话接线（issue #1275）：写入口
            // 已持有的连接包成作用域会话交给编排——编排的读写只经会话，网络
            // I/O 在会话之外；本实现直通同一连接，锁跨度不变。
            let mut progress = progress_to_emitter(&progress_app);
            let session = WriteEntrySession { conn };
            do_incremental_sync(&session, &mut progress).map(|result| {
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
    use crate::sync::FundNavProgress;
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

    /// 壳层接线证明（issue #1275）：命令壳把写入口持有的连接包成作用域会话
    /// 交给编排——会话交出的就是这把连接（经会话写入、原连接立即可读）。
    /// 直通实现下两者本就是同一句柄；删除 [`WriteEntrySession`] 即无会话
    /// 可交给编排，编译红即接线证明的负向半边（ADR-0087 断言强度）。
    #[test]
    fn command_shell_hands_write_entry_connection_to_orchestration_via_session() {
        let conn = crate::test_support::open();
        let session = WriteEntrySession { conn: &conn };

        session
            .with_connection(|c| {
                use crate::error::AppError;
                c.execute(
                    "INSERT INTO categories (id, name, kind, created_at, updated_at, version, device_id) \
                     VALUES ('cat-session', '会话', 'expense', ?1, ?1, 1, 'device-1')",
                    [crate::test_support::FIXED_NOW],
                )
                .map_err(AppError::from)?;
                Ok(())
            })
            .expect("经会话的写入应成功");

        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM categories WHERE id = 'cat-session'",
                [],
                |r| r.get(0),
            )
            .expect("查询应成功");
        assert_eq!(count, 1, "会话交出的应是写入口持有的那把连接");
    }

    /// 壳层接线证明（issue #897 / ADR-0095；先例：信号发射两层测试——映射层
    /// 钉「谁发什么」，本测试钉「命令壳把编排回调接到事件发射」的接线半边，
    /// 编排回调的触发时序归 sync 域进度序列测试）：`progress_to_emitter` 把
    /// 编排的进度回调接到发射器，载荷形状与顺序保持不变（含基金页级明细）。
    #[test]
    fn command_shell_wires_orchestration_progress_to_emitter() {
        let emitter = RecordingEmitter::default();
        let mut progress = progress_to_emitter(&emitter);

        progress(SyncProgress {
            done: 0,
            total: 100,
            fund: None,
        });
        progress(SyncProgress {
            done: 37,
            total: 100,
            fund: None,
        });
        progress(SyncProgress {
            done: 37,
            total: 100,
            fund: Some(FundNavProgress {
                code: "110022".into(),
                page: 3,
                pages: 25,
            }),
        });
        progress(SyncProgress {
            done: 100,
            total: 100,
            fund: None,
        });

        assert_eq!(
            *emitter.0.lock().unwrap(),
            vec![
                SyncProgress {
                    done: 0,
                    total: 100,
                    fund: None,
                },
                SyncProgress {
                    done: 37,
                    total: 100,
                    fund: None,
                },
                SyncProgress {
                    done: 37,
                    total: 100,
                    fund: Some(FundNavProgress {
                        code: "110022".into(),
                        page: 3,
                        pages: 25,
                    }),
                },
                SyncProgress {
                    done: 100,
                    total: 100,
                    fund: None,
                },
            ],
            "编排回调逐次直达发射器，载荷形状与顺序保持不变（含基金页级明细）"
        );
    }
}
