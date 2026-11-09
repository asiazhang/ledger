//! NSH 壳层机制：壳层统一写入口与统一读入口（本壳对应面）。
//!
//! 旧壳同款机制住根包 `src-tauri/src/shell_support/`（spec #1086 P5 / #1108 壳
//! 层收敛；ADR-0111：只被壳层消费的机制住壳层）。#2081 起 AI 导入 HTTP 壳
//! （`api_server`）迁居本壳，其写端点经统一写入口、读端点经统一读入口的既有
//! 接线随宿主迁居——本模块承载 NSH 侧的同构实现（ADR-0145 双壳并存期两侧同构
//! 代价条已接受；切轨 #2083 后旧壳副本随根包退役，#2077 为壳层机制 NSH 对应
//! 的专项收敛票）。
//!
//! 只迁 **api_server 消费的形态**：整段写入口 [`write_entry`]（事务、提交点置
//! 脏、信号内化单点，ADR-0073）+ [`Outcome`] 证据包装（ADR-0073 决策 2）+ 整段
//! 读入口 [`read_entry`]（ADR-0104）。旧壳的分段形态（`write_entry_segmented(_async)`
//! / `SegmentLock`，同步轮次长任务编排专用）在本壳无消费面——本壳同步编排的
//! 收尾裁决已有自己的等价半边（[`crate::boot::sync_instrument_info`]，
//! `FacadeWriteSession`），不引入第二份分段机制。
//!
//! 语义锚（与旧壳逐字节一致，详见旧壳 `shell_support/write_entry.rs` 模块文档）：
//! - **发射时序**：事务提交成功后发射（写失败早退不发）；发射走
//!   [`SignalEmitter::post`] 投递接缝，发射失败静默忽略、不影响写结果（ADR-0044）。
//!   「发不发、发哪个」判定单点仍是 [`ledger_infra::signals::signals_for`]；
//!   生产发射器由 [`crate::http_host`] 接线（NSH 泵发射器，ADR-0112 决策 5 注册
//!   点反转——旧壳的 `AppHandle` 投递实现是 tauri 类型件，本壳不链接 tauri）；
//! - **结果证据**：[`Outcome::Silent`]（零证据）/ [`Outcome::Evidenced`]（携带
//!   [`WriteEvidence`]），条件信号的「条件」一半由证据承载；
//! - **发射器参数归一**（ADR-0073 决策 3）：`Option<&dyn SignalEmitter>`——
//!   `None` 跳过发射（两侧共有的既有测试态语义）；
//! - **span 归因串**保留 `&'static str` 参数：HTTP 壳传 `"METHOD /path"` 端点键，
//!   SQL 耗时日志逐字节不变（ADR-0009 / ADR-0068 零感知）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use rusqlite::Connection;

use ledger_infra::db::{DbReadHandle, DbWriteHandle};
use ledger_infra::error::Result;
use ledger_infra::events::SignalEmitter;
use ledger_infra::signals::{WriteEvidence, WriteOp, emit_for};

/// 壳层统一写入口的结果证据包装（ADR-0073 决策 2）：[`write_entry`] 的业务闭包
/// 统一返回 `Result<Outcome<T>>`——单形态，证据只能从返回值来，「携带证据」是
/// 类型要求而非调用方自觉。零证据端点的代价是一个 [`Outcome::Silent`] 构造器。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T> {
    /// 零证据：信号完全由写操作身份的静态映射行决定（含刻意零信号身份）。
    Silent(T),
    /// 携带结果证据：条件信号的「条件」一半（价格写入 / 黑洞即建 / 商户即建，
    /// 见 [`WriteEvidence`]），由映射单点与身份合成「发不发」。
    Evidenced(T, WriteEvidence),
}

/// 壳层统一写入口（ADR-0073 决策 1，旧壳同款整段形态）：按序组合门面写作业
/// （写 DB 线程 + span 传播 + 连接层统一写入口的已持锁形态：事务、提交点置脏）
/// → `signals_for` → 发射。
///
/// - `span`：SQL 归因串（`&'static str`，HTTP 壳传契约端点键 `METHOD /path`）；
/// - `emitter`：`None` 跳过发射（两侧既有测试态语义）；
/// - 闭包业务错误原样传播、闭包 panic 归一化为 [`ledger_infra::error::AppError::Io`]
///   （与 ADR-0069 先例同形）；写失败早退不发信号；
/// - 发生在写事务提交成功之后、调用线程上（与旧壳「await 后发射」逐点同位）。
pub async fn write_entry<T, F>(
    span: &'static str,
    db: DbWriteHandle,
    emitter: Option<&dyn SignalEmitter>,
    op: WriteOp,
    f: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<Outcome<T>> + Send + 'static,
{
    let (value, evidence) = db
        .run(span, move |conn| match f(conn)? {
            Outcome::Silent(value) => Ok((value, WriteEvidence::None)),
            Outcome::Evidenced(value, evidence) => Ok((value, evidence)),
        })
        .await?;
    // 事务提交成功后发射（映射单点判定，ADR-0044）：emit_for = signals_for +
    // emit_all，发射失败静默忽略，不影响写结果。
    if let Some(emitter) = emitter {
        emit_for(emitter, op, evidence);
    }
    Ok(value)
}

/// 壳层统一读入口（ADR-0104 决策 2，旧壳同款）：把读闭包交给门面读句柄，在
/// 门面读 DB 线程上执行。
///
/// - `span`：SQL 归因串（`&'static str`，HTTP 壳传契约端点键）；
/// - 闭包业务错误原样传播、闭包 panic 归一化为 [`ledger_infra::error::AppError::Io`]
///   （门面侧 `catch_unwind` + 未提交事务回滚，ADR-0125 决策 3）；锁中毒归一化
///   为 [`ledger_infra::error::AppError::Db`]。
///
/// 读路径无置脏维度（ADR-0032 置脏豁免单点不动，ADR-0104 关联）；读连接是
/// 门面读线程（长写事务不把读排在后面，ADR-0117 读路径不进写者闸门）。
pub async fn read_entry<T, F>(span: &'static str, db: DbReadHandle, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T> + Send + 'static,
{
    db.run(span, f).await
}

#[cfg(test)]
mod tests {
    use ledger_infra::events::{LEDGER_CHANGED, PRICES_CHANGED};
    use ledger_infra::signals::WriteEvidence;
    use ledger_infra::test_utils::{GATED_TIMEOUT, GatedEmitter};
    use std::sync::{Arc, Mutex};

    use super::*;

    /// 内存库夹具（统一测试工厂建库）：写读两槽同指一连接（与 `DbState`
    /// 字段形状一致），写/读句柄经槽对具名访问器取——测试世界构造保持
    /// 「裸库 + 状态」原形（ADR-0125 决策 4）。
    fn fixture() -> (ledger_infra::db::DbSlotPair, GatedEmitter) {
        let conn = Arc::new(Mutex::new(crate::test_support::open()));
        (
            ledger_infra::db::DbSlotPair::new(conn.clone(), conn),
            GatedEmitter::gated(),
        )
    }

    /// 身份传递：静态映射行身份（无证据）按 signals_for 发对应信号（旧壳
    /// write_entry 同名测试的本壳同构版——统一写入口迁居后行为零变化）。
    #[test]
    fn identity_drives_static_signal_row() {
        let (slots, emitter) = fixture();
        let write = slots.write_handle();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(write_entry(
                "test",
                write,
                Some(&emitter),
                WriteOp::CreateAccount,
                move |_conn| Ok(Outcome::Silent("id")),
            ))
            .unwrap();
        assert_eq!(
            emitter.posted(),
            vec![LEDGER_CHANGED],
            "参考写入身份应发参考失效信号"
        );
    }

    /// 证据传递：Evidenced + 条件证据为真 → 按身份与证据发价格信号；条件为假
    /// → 零信号（「发不发」判定在映射单点，入口只传递）。
    #[test]
    fn evidenced_evidence_reaches_signal_mapping() {
        let (slots, emitter) = fixture();
        let write = slots.write_handle();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(write_entry(
                "test",
                write,
                Some(&emitter),
                WriteOp::SyncInstrumentInfo,
                move |_conn| {
                    Ok(Outcome::Evidenced(
                        "done",
                        WriteEvidence::PriceWritten(true),
                    ))
                },
            ))
            .unwrap();
        assert_eq!(
            emitter.posted(),
            vec![PRICES_CHANGED],
            "价格写入应发价格信号"
        );
    }

    /// 发射器 None：跳过发射（两侧既有测试态语义），写结果不受影响。
    #[test]
    fn none_emitter_skips_emission() {
        let (slots, emitter) = fixture();
        let write = slots.write_handle();
        let value = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(write_entry(
                "test",
                write,
                None,
                WriteOp::CreateAccount,
                move |_conn| Ok(Outcome::Silent("id")),
            ))
            .unwrap();
        assert_eq!(value, "id");
        assert!(emitter.posted().is_empty(), "None 发射器不应发信号");
    }

    /// 业务错误原样传播（不二次包装），且失败早退不发信号。
    #[test]
    fn business_error_propagates_verbatim_without_emission() {
        let (slots, emitter) = fixture();
        let write = slots.write_handle();
        let err = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(write_entry::<(), _>(
                "test",
                write,
                Some(&emitter),
                WriteOp::CreateAccount,
                move |_conn| Err(ledger_infra::error::AppError::Invalid("boom".into())),
            ))
            .unwrap_err();
        assert!(
            matches!(err, ledger_infra::error::AppError::Invalid(ref m) if m == "boom"),
            "业务错误应原样传播，实际 {err:?}"
        );
        assert!(emitter.posted().is_empty(), "写失败不应发信号");
    }

    /// 读入口：闭包拿到可用连接，读已提交的真实行（连接机制内化但真实可用）。
    #[test]
    fn read_entry_closure_receives_usable_connection() {
        let (slots, _emitter) = fixture();
        let read = slots.read_handle();
        let balance: i64 = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(read_entry("test", read, move |c| {
                c.query_row("SELECT 12345", [], |r| r.get(0))
                    .map_err(ledger_infra::error::AppError::from)
            }))
            .unwrap();
        assert_eq!(balance, 12345, "闭包应能读到已提交的行");
    }

    /// 写路径「不被发射阻塞」的外部保证在本壳副本上同样成立（spec #366 契约）：
    /// 发射器阻塞期间写路径仍及时返回，放行后信号最终到达、不丢失。
    #[test]
    fn write_entry_is_not_blocked_by_gated_emitter() {
        let (slots, emitter) = fixture();
        let write = slots.write_handle();
        // 写结果经通道回传：recv_timeout 是本测试的超时上界——发射步骤若被
        // 退化成「同步等送达」，闸门关闭期间写路径卡死，本测试超时红。
        let (tx, rx) = std::sync::mpsc::channel();
        // 发射器可克隆（写线程与测试各持一份，GatedEmitter 同款约定）。
        let worker_emitter = emitter.clone();
        let worker = std::thread::spawn(move || {
            let result = tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(write_entry(
                    "test",
                    write,
                    Some(&worker_emitter),
                    WriteOp::CreateAccount,
                    move |_conn| Ok(Outcome::Silent("id")),
                ));
            tx.send(result).expect("回报写结果应成功");
        });
        // 写路径不及时返回（> 闸门超时仍持着）即本测试在 recv 超时后红。
        let result = rx
            .recv_timeout(GATED_TIMEOUT)
            .expect("发射器阻塞期间写路径应及时返回（发射非阻塞契约）")
            .expect("写结果应成功");
        assert_eq!(result, "id");
        emitter.open_gate();
        assert_eq!(emitter.wait_delivered(), vec![LEDGER_CHANGED]);
        worker.join().expect("写线程应正常收尾");
    }
}
