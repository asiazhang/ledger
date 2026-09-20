//! 标的信息同步 IPC 命令壳（issue #89；#407 域目录化后压平为单文件纯壳；
//! issue #827 命令改名 `sync_holding_prices` → `sync_instrument_info`）。
//!
//! `sync_instrument_info` 写路径与信号经壳层统一写入口
//! [`crate::shell_support::write_entry::write_entry`]（ADR-0073）：仪式内化单点，证据随闭包
//! 返回必达。标的全量同步命令/中断命令与进度事件已随 ADR-0081 决策 3 整体
//! 退役（issue #698）：股票字典修正归「按代码查询/创建带回权威名称」。

// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::shell_support::write_entry::{Outcome, SegmentedFailure, write_entry_segmented_async};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::{WriteEvidence, WriteOp};
use ledger_market_sync::{
    FacadeWriteSession, FxSyncChannels, FxSyncReport, ProgressEmitter, SyncFetchChannels,
    SyncInstrumentInfoResult, SyncProgress, WriteWitness, do_incremental_sync_channels,
    sync_fx_rates,
};

/// 同步网络通道注入接缝（issue #1276）：生产**不管理**本状态（命令走生产通道
/// 束），集成测试 manage 本状态并装入桩通道束，使「同步真实在途」可确定复现
/// ——门控的抓取闭包发生在会话与作业之外，读命令可读性测试与分段形态在注入
/// 桩下原样运行。形态沿用既有注入体例（`FundQuoteFetcher` / `StockQuoteFetcher`
/// 同款：缺省生产实现、测试注入桩）。异步互斥体（issue #1412）：通道束引用在
/// `.await` 之间存活（闭包 async 化后 `std` 守卫跨 `await` 会让 future 失去
/// `Send`；tokio 互斥体无中毒语义，取锁不可失败）。
pub struct SyncChannelsSlot(pub Arc<tokio::sync::Mutex<SyncFetchChannels>>);

/// 生产会话接线（issue #1275 接缝，#1412 async 形态）：门面写槽裸作业会话交给
/// 编排（[`FacadeWriteSession`] = [`ScopedSession`] 的生产实现）。编排每次取连接
/// 都经这里短暂投递一次门面作业、`await` 结果即还；分钟级网络等待发生在分段与
/// 分段之间（会话之外，`await` 表达）——同步在途时连接对其他命令保持可取
///（父 spec #1274 路线 A 的落点）。独立成函数与 [`progress_to_emitter`] 同款：
/// 命令体一行调用，接线点可命名、可被测试钉住。`span` = 作业的 SQL 归因串
///（== 命令名，ADR-0073 决策 4 同口径）。
pub(crate) fn facade_session(
    write: ledger_infra::db::DbWriteHandle,
    span: &'static str,
) -> FacadeWriteSession {
    FacadeWriteSession::new(write, span)
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
/// 跳过；空库返回明确提示而非报错。异步执行（async 命令任务；网络等待以
/// `await` 表达，DB 取用经门面作业，均不阻塞调用线程）；返回
/// 结果统计（同步 N 只 / 跳过 M 只），前端据此轻量提示。实际写入价格**或**
/// 名称时发价格失效信号（ADR-0031；成败同判，issue #1277）：零变化（空库/
/// 全部跳过/基金无新净值且名称无变化/失败且未写过）为库内零变化，不广播、
/// 不置脏——分段形态逐段 autocommit，中途失败的运行已落库的写入仍然生效
/// （失败 ≠ 未写过），收尾裁决按实际写入归一。
///
/// 同步全程发确定进度事件（issue #897 / ADR-0095）：编排的进度回调经
/// [`progress_to_emitter`] 接到 `ledger:instrument-sync-progress` 带 payload 事件
///（标的级 `{ done, total }`，基金深回填期间另带页级明细 `fund`，issue #1061；
/// 经 [`ProgressEmitter`] 非阻塞投递主线程）；进度事件不是失效信号，不影响下方
/// 价格失效信号的判定与发射条件。
///
/// 「是否发」判定已于 #333 归一化进 signals 映射单点（`signals_for` +
/// [`WriteEvidence::PriceWritten`]，ADR-0044）：入口只把终态归一化为证据——
/// 成败同判（issue #1277）：编排的写入见证（[`WriteWitness`]，每个实际写入点
/// 标记，跨分段累积）同时是成功与失败终态的证据源——实际写过即置脏即发信号，
/// 零写入（空库/全部跳过/失败且未写过）不置脏不广播；失败的业务错误在发射后
/// 原样上抛，用户可见语义不变。分段取连接、整体裁决形态（issue #1276；
/// async 形态见 #1412）：仍是恰好一处写入口调用、一个写操作身份；跨分段的
/// 「是否实际写过」由见证器累积，在收尾裁决点一次性置脏、一次发射。
#[tauri::command]
pub async fn sync_instrument_info<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
) -> Result<SyncInstrumentInfoResult> {
    let write = db.write_handle();
    // 进度发射器归进写闭包自有的一份句柄：`app` 同时被下方发射器参数借用，
    // 闭包（Send + 'static）捕获克隆件（issue #897）。
    let progress_app = app.clone();
    // 通道束换装（issue #1276）：测试注入桩优先（`SyncChannelsSlot` 管理态），
    // 生产默认生产通道束（每次同步构造，与先前整段形态同口径）。#1411 起抓取
    // 闭包为异步形态（reqwest async 客户端构造与等待均无运行时线程约束），命令体
    // （async 命令任务）内构造安全；#1412 起编排直接在本命令任务上 await，分段
    // 取连接经会话接缝投门面作业（不再经 run_db 阻塞线程池）。
    let channels: Arc<tokio::sync::Mutex<SyncFetchChannels>> =
        match app.try_state::<SyncChannelsSlot>() {
            Some(slot) => slot.0.clone(),
            None => Arc::new(tokio::sync::Mutex::new(SyncFetchChannels::production()?)),
        };
    let session_write = write.clone();
    write_entry_segmented_async(
        "sync_instrument_info",
        write,
        Some(&app),
        WriteOp::SyncInstrumentInfo,
        // 行情/汇率/历史/名称落库成功即置脏，提交点写时顺带到期检查（ADR-0032，
        // #246 审计补齐）；分段取连接、整体裁决：置脏与信号在收尾点各一次。
        async move {
            // 进度接线（issue #897）：编排进度回调 → 进度事件发射（非阻塞投递，
            // 发射失败静默，不影响同步结果）。会话接线（issue #1275/#1276，
            // #1412 async 形态）：门面写槽裸作业会话交给编排——编排的读写只经
            // 会话短暂投递门面作业，网络 I/O（await）在作业之外。
            let session = facade_session(session_write, "sync_instrument_info");
            let mut progress = progress_to_emitter(&progress_app);
            let mut channels = channels.lock().await;
            // 写入见证（issue #1277）：编排的每个实际写入点标记，跨分段累积；
            // 成败同判的同一份证据源（成功时与结果统计 any_written() 同口径，
            // 失败时结果统计随错误丢失，见证器是唯一幸存的证据）。
            let mut witness = WriteWitness::default();
            do_incremental_sync_channels(&session, &mut channels, &mut progress, &mut witness)
                .await
                .map(|result| {
                    // 证据 = 价格或名称实际写入（issue #827）：名称刷新同样让
                    // 标的列表失真，与价格写入同路计入「数据变了」。
                    let evidence = WriteEvidence::PriceWritten(witness.any_written());
                    Outcome::Evidenced(result, evidence)
                })
                .map_err(|error| SegmentedFailure {
                    // 失败收尾同样按实际写入裁决（#1277）：证据随失败必达，
                    // 收尾点置脏并发信号后错误原样上抛。
                    error,
                    evidence: WriteEvidence::PriceWritten(witness.any_written()),
                })
        },
    )
    .await
}

/// IPC 命令：手动同步汇率一次（issue #1545 设置页「同步汇率」入口）：门面写槽裸
/// 作业会话交给汇率同步编排（[`sync_fx_rates`]，#1275 会话接缝同款；生产通道束
/// 接 ECB 官方站，取数深度由窗口判据分派——深度未达走全量回填、已达走 90 天
/// 增量），返回同步报告（是否回填 + 覆盖区间 / 条数，前端结果面）。失败原因码化
/// 三态互不吞并（fx.source-unreachable / fx.source-no-data /
/// fx.source-malformed），前端按码本地化后可分辨。
///
/// 不经 [`write_entry`](crate::shell_support::write_entry)：ECB 汇率落库是可重建
/// 缓存的自动采集——不产同步 op（ADR-0019 修订记录）、不发失效信号（当期汇率表
/// 不在 ledger:prices-changed 覆盖内，ADR-0031 映射）、不置脏（余额缓存是本位币
/// 口径，汇率变化不涉及）。写连接取用经会话裸作业（与后台车道同款形态），
/// 网络等待在会话之外以 await 表达（慢闭包纪律）。
#[tauri::command]
pub async fn sync_exchange_rates(db: State<'_, DbState>) -> Result<FxSyncReport> {
    let session = facade_session(db.write_handle(), "sync_exchange_rates");
    let mut channels = FxSyncChannels::production()?;
    sync_fx_rates(&session, &mut channels).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_market_sync::{FundNavProgress, ScopedSession};
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

    /// 壳层接线证明（issue #1275/#1276 换装，#1412 async 形态）：命令壳把门面
    /// 写槽裸作业会话交给编排——经会话写入落库、作业后立即可读（分段取连接：
    /// 每次短暂投递门面作业、用完即还）。删除 [`facade_session`] 即无会话可交
    /// 给编排，编译红即接线证明的负向半边（ADR-0087 断言强度）；作业执行线程
    /// 归属（门面写 DB 线程）由 write_entry 入口测试钉住，不在此重复。
    #[test]
    fn command_shell_hands_write_entry_connection_to_orchestration_via_session() {
        let conn = crate::test_support::open();
        let shared = std::sync::Arc::new(std::sync::Mutex::new(conn));
        // 会话经门面写句柄构造（issue #1412）：连接槽由门面为整个作业持有，
        // 命令壳不再自取连接槽。
        let handle = ledger_infra::db::DbSlotPair::new(
            std::sync::Arc::clone(&shared),
            std::sync::Arc::clone(&shared),
        )
        .write_handle();
        let session = facade_session(handle, "sync_instrument_info");

        tauri::async_runtime::block_on(session.with_connection(|c| {
            use ledger_infra::error::AppError;
            let name = "会话";
            c.execute(
                "INSERT INTO categories (id, name, kind, created_at, updated_at, version, device_id) \
                 VALUES ('cat-session', ?1, 'expense', ?2, ?2, 1, 'device-1')",
                rusqlite::params![name, crate::test_support::FIXED_NOW],
            )
            .map_err(AppError::from)?;
            Ok(())
        }))
        .expect("经会话的写入应成功");

        // 分段语义：作业返回后连接已归还，另一持锁方能取到同一连接；写入
        // 在门面写 DB 线程执行（线程名是可观察坐标，ADR-0125 决策 1）。
        let count: i64 = shared
            .lock()
            .expect("段后锁应可取（短暂作业已还）")
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
