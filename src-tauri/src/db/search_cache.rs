//! 进程内搜索候选缓存（issue #493 / ADR-0027 修订记录预留手段的兑现）。
//!
//! V018 两段式的第一段（SQL index-only 流式扫描 50 万候选 + Rust 语义过滤）
//! 在 perf-bench CI 实测 p95 313ms，超 ADR-0068 门禁 200ms；本模块把第一段
//! 的候选集物化为进程内数组（列表序的 id + note + note_pinyin + 三个引用
//! 列），关键字纯路径改扫内存数组（第二段回表、语义契约、字典口径全部不变，
//! 见 `transaction::search`）。一致性模型为 spec #489 B3 预留方案原文——
//! **写后脏标记 + 惰性重建**：
//!
//! - 失效挂点全集（任何 transactions 表写入必经其一，多处挂同一 [`invalidate`]，
//!   幂等且 O(1)）：
//!   1. `db::write` 提交点 `after_commit`（产品写路径单点，ADR-0032 接缝；
//!      行为编排、批量导入、定时执行、投资买卖、退款链、余额调整全部经此）；
//!   2. `writer::insert_row` / `writer::update_row` / 行为层软删 / 批次去重
//!      回写（域函数直调形态：测试与不经 wrapper 的内部路径，与 note_pinyin
//!      同写维护同款接缝纪律）；
//!   3. 连接工厂 `open_connection` / `open_in_memory`（新连接身份：恢复、
//!      搬迁等重开连接的路径由此覆盖；同时杜绝「新连接复用旧地址误命中旧
//!      快照」）。
//! - 连接身份判别：槽记录连接指针值，命中须同连接——进程内多连接（测试）
//!   各自重建，互不串快照；产品为全局单连接长生命周期，稳定命中。
//! - 过度失效无害：非交易写（账户/分类/商户/预算等）经 wrapper 也触发失效，
//!   代价只是写后首次搜索多一次重建，正确性不受影响。
//! - 分层（V017 余额/净资产缓存先例）：本模块在基础设施，行结构「与搜索域
//!   第一段候选同形而独立定义」，不反向依赖域类型；匹配语义与口径全部留在
//!   `transaction::search`。锁序恒定「连接锁 → 本模块槽锁」（产品调用方均
//!   持连接锁进入），无死锁面。

use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::{AppError, Result};

/// 第一段候选行（与 `transaction::search` 第一段 SQL 的最小列一一对应）。
pub struct CandidateRow {
    pub(crate) id: String,
    pub(crate) note: Option<String>,
    /// V018 拼音冗余列（重建前由搜索域先行惰性回填；NULL 透传，匹配侧现算兜底）。
    pub(crate) note_pinyin: Option<String>,
    pub(crate) account_id: String,
    pub(crate) merchant_id: Option<String>,
    pub(crate) category_id: Option<String>,
}

/// 缓存槽：单连接的候选快照（产品单连接；测试多连接经连接身份互斥）。
enum SlotState {
    Empty,
    Ready {
        conn_key: usize,
        rows: Vec<CandidateRow>,
    },
}

static SLOT: Mutex<SlotState> = Mutex::new(SlotState::Empty);

/// 锁中毒容忍：缓存是可重建的派生数据，无跨线程不变量，中毒后原样接管。
fn lock_slot() -> std::sync::MutexGuard<'static, SlotState> {
    SLOT.lock().unwrap_or_else(|e| e.into_inner())
}

/// 写后失效：清空缓存槽（幂等、O(1)、不持有连接锁——调用方处于连接锁内时
/// 本函数同样安全）。失效挂点全集见模块注释。
pub fn invalidate() {
    *lock_slot() = SlotState::Empty;
}

/// 取（必要时重建）当前连接的候选快照，持槽锁借用执行 `f`。
///
/// 重建为一次单表快照查询（软删口径 + 列表序，与第一段 SQL 同源同序）；
/// 调用方（搜索域）负责在进入本函数前完成 note_pinyin 惰性回填。
pub(crate) fn with_shared_rows<R>(
    conn: &Connection,
    f: impl FnOnce(&[CandidateRow]) -> R,
) -> Result<R> {
    let conn_key = (conn as *const Connection) as usize;
    let mut slot = lock_slot();
    let need_rebuild = !matches!(
        &*slot,
        SlotState::Ready {
            conn_key: k,
            ..
        } if *k == conn_key
    );
    if need_rebuild {
        let rows = rebuild(conn)?;
        *slot = SlotState::Ready { conn_key, rows };
    }
    match &*slot {
        SlotState::Ready { rows, .. } => Ok(f(rows)),
        // 结构上不可达（need_rebuild 为 false 时槽必为同连接 Ready）；
        // 按 ADR-0060 纪律不以 panic 兼「不可能」，返回码化错误。
        SlotState::Empty => Err(AppError::Db("候选缓存槽意外为空".into())),
    }
}

/// 重建快照：单表最小列全量读取，软删口径与列表序与第一段 SQL 完全一致
/// （`INDEXED BY` 钉定 V018 搜索覆盖索引：index-only、零临时 B-tree，先例
/// 同 `stage1_sql` 的钉定纪律）。
fn rebuild(conn: &Connection) -> Result<Vec<CandidateRow>> {
    let mut stmt = conn.prepare(
        "SELECT id,note,note_pinyin,account_id,merchant_id,category_id \
         FROM transactions INDEXED BY idx_transactions_note_search \
         WHERE is_deleted = 0 \
         ORDER BY date DESC, created_at DESC, id DESC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(CandidateRow {
                id: row.get(0)?,
                note: row.get(1)?,
                note_pinyin: row.get(2)?,
                account_id: row.get(3)?,
                merchant_id: row.get(4)?,
                category_id: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
