//! 交易批量写入模块 `TransactionBatch`（issue #53 / #63）。
//!
//! 承载全部「批量写入交易」的编排语义：事务 `BEGIN/COMMIT/ROLLBACK`、逐条 INSERT +
//! 回写 `dedup_hash`/`idempotency_key`、批次汇总日志（ADR-0009 决策 #5 / issue #45）。
//!
//! **为何是「批量编排」而非「导入专属」模块**：`run` 同时服务 HTTP 批量导入
//! （`POST /api/v1/transactions/batch`，`dedup=true`）与 IPC 前端批量创建
//! （`create_transactions`，`dedup=false`）——它们共享同一段编排。真正的轴是
//! 「**批量编排 vs 单条落库**」，去重身份只是 `dedup` 注入的选项，不是「导入」概念的固有属性。
//!
//! **边界**：单笔落库（行为层创建编排入口 `transactions::behavior::create`，含 buy/sell
//! 持仓副作用，issue #228）不在本模块重演，本模块只做编排、不重复列映射/折算逻辑；去重身份判定
//! （T1/issue #62 的 `dedup_identity`，幂等键优先 / 内容哈希兜底，ADR-0010 冻结契约
//! 编码在 `DedupIdentity` 类型里而非散在 if 分支）随去重逻辑一并收进本模块。
//!
//! **对外契约**：`run` 返回 `Vec<CreateTransactionResult>`，与 HTTP/IPC 响应形状一致；
//! 本重构只做内部重组，不改响应形状、不改事务/去重语义（原命令模块中的转发层已随
//! 收缩批次（issue #67）删除，`run` 是批量写入的唯一入口）。

#[cfg(test)]
mod tests;

use std::time::Instant;

use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::commands::transactions::create_transaction_internal;
use crate::error::{AppError, Result};
use crate::models::{CreateTransactionResult, TransactionInput};

/// 批量写入交易的深度模块：向调用方暴露一个稳定入口 `run`，承载全部批量编排语义。
#[derive(Debug, Clone, Copy, Default)]
pub struct TransactionBatch;

impl TransactionBatch {
    /// 批量写入一笔或多笔交易（事务 / 去重 / 汇总日志 / 索引消费）。
    ///
    /// 去重身份判定（T1/issue #62 的 `dedup_identity`，幂等键优先 / 内容哈希兜底）只在
    /// `dedup=true` 时生效，`dedup=false` 直接落库；单条校验失败（`AppError::Invalid`）
    /// 返回 `success:false`+`error` 且不影响同批其他交易，提交失败则整批回滚并在回滚路径
    /// 打批次汇总日志。
    pub fn run(
        conn: &Connection,
        inputs: Vec<TransactionInput>,
        dedup: bool,
    ) -> Result<Vec<CreateTransactionResult>> {
        let started = Instant::now();
        let total = inputs.len();
        conn.execute("BEGIN", [])?;
        let mut results = Vec::with_capacity(total);
        // 失败条数：累计到批次汇总日志（成功路径=无效行数；回滚路径含触发回滚的那条）。
        let mut failed = 0usize;
        for input in inputs {
            // 客户端幂等键：带键时作为去重身份（内容无关），无键时 None 走内容哈希兜底。
            let idempotency_key = input.idempotency_key.clone();
            // 去重身份判定用 T1/issue #62 的 `dedup_identity`：带键按键查（走部分唯一索引）、
            // 无键回退内容哈希；ADR-0010 的契约编码在 `DedupIdentity` 类型里，不散在 if 分支。
            // `New` 携带内容哈希供落库回写 `dedup_hash` 列，避免重复计算。
            // 单条写入（含 buy/sell 的持仓副作用路径）由行为层创建编排入口
            // `behavior::create`（issue #228 / ADR-0033）承担：本函数持有外层批次事务，
            // 入口以嵌套模式加入（失败直接返回错误、回滚归本层），其交易行落库
            // 已收口到 `transaction::writer` 接缝（issue #60）：列映射与审计字段统一由
            // writer 生成，此处不重复；去重身份（幂等键/内容哈希）仍在本批次编排层
            // 判定与回写，不沉入 writer。
            let dedup_hash = if dedup {
                match dedup_identity(conn, &input)? {
                    DedupIdentity::Existing { id } => {
                        results.push(CreateTransactionResult {
                            success: true,
                            duplicate: true,
                            // 冻结契约即类型：幂等键命中回传已有 id，内容哈希命中回传 id:None（不回归）。
                            id,
                            error: None,
                        });
                        continue;
                    }
                    DedupIdentity::New { dedup_hash } => dedup_hash,
                }
            } else {
                compute_dedup_hash(&input)
            };
            match create_transaction_internal(conn, input) {
                Ok(id) => {
                    if let Err(e) = conn.execute(
                        "UPDATE transactions SET dedup_hash=?1, idempotency_key=?2 WHERE id=?3",
                        rusqlite::params![dedup_hash, idempotency_key, id],
                    ) {
                        conn.execute("ROLLBACK", [])?;
                        failed += 1;
                        log_batch_summary(started, total, failed, false);
                        return Err(e.into());
                    }
                    results.push(CreateTransactionResult {
                        success: true,
                        duplicate: false,
                        id: Some(id),
                        error: None,
                    });
                }
                Err(AppError::Invalid(msg)) => {
                    failed += 1;
                    results.push(CreateTransactionResult {
                        success: false,
                        duplicate: false,
                        id: None,
                        error: Some(msg),
                    });
                }
                Err(e) => {
                    conn.execute("ROLLBACK", [])?;
                    failed += 1;
                    log_batch_summary(started, total, failed, false);
                    return Err(e);
                }
            }
        }
        if let Err(e) = conn.execute("COMMIT", []) {
            // COMMIT 失败：尝试回滚清理残留（错误路径同样记录批次汇总）。
            let _ = conn.execute("ROLLBACK", []);
            log_batch_summary(started, total, failed, false);
            return Err(e.into());
        }
        // 汇总行在 COMMIT 后立即打一条（ADR-0009 决策 #5 / issue #45）：
        // 数据已提交，无论后续置脏检查成败，批次都应有一条可观测的汇总行。
        // 批量导入提交成功（issue #126）：逐行 insert 时已置脏；此处再调一次补上
        // 提交点的「写时顺带检查」——行内检查因处于事务中而推迟到本处执行。
        crate::auto_backup::on_write(conn);
        log_batch_summary(started, total, failed, true);
        Ok(results)
    }
}

/// 计算导入去重哈希：`sha256("date|kind|amount_cents|currency_code|account_id|to_account_id")`。
/// `to_account_id` 缺省拼空串；刻意排除 note/category（AI 生成文本非确定性，会让哈希漂移）。
pub fn compute_dedup_hash(input: &TransactionInput) -> String {
    let to_account_id = input.to_account_id.as_deref().unwrap_or("");
    let payload = format!(
        "{}|{}|{}|{}|{}|{}",
        input.date,
        input.kind.as_str(),
        input.amount_cents,
        input.currency_code,
        input.account_id,
        to_account_id
    );
    let digest = Sha256::digest(payload.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// 单条导入交易的去重身份判定：新写还是命中已有（issue #62，prefactor）。
///
/// ADR-0010 的冻结契约被编码为类型而非散在 if 分支：命中已有时，`Existing.id`
/// 即契约要求回传的值——幂等键命中（内容无关）为 `Some(已有 id)`；内容哈希兜底
/// 命中为 `None`（维持冻结行为 `id: None`，不回归）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DedupIdentity {
    /// 未命中任何未删除交易，应新写；携带已计算好的内容哈希（落库时回写 `dedup_hash` 列）。
    New { dedup_hash: String },
    /// 命中已有未删除交易。
    Existing {
        /// 按冻结契约回传给调用方的 id：幂等键命中为 Some(已有 id)，内容哈希命中为 None。
        id: Option<String>,
    },
}

/// 判定一条导入交易的去重身份：新写还是命中已有（issue #62）。
///
/// 带客户端幂等键时按键查：内容无关、命中优先走部分唯一索引
/// `idx_transactions_idempotency_key`（非全表扫描）；无键时回退确定性内容哈希
/// （`compute_dedup_hash`，排除 note/category）。
pub fn dedup_identity(conn: &Connection, input: &TransactionInput) -> Result<DedupIdentity> {
    let dedup_hash = compute_dedup_hash(input);
    // 带客户端幂等键：按键查（内容无关、走部分唯一索引命中）；幂等键命中回传已有 id。
    if let Some(key) = &input.idempotency_key {
        let hit: Option<String> = conn
            .query_row(
                "SELECT id FROM transactions \
                 WHERE idempotency_key=?1 AND is_deleted=0 LIMIT 1",
                rusqlite::params![key],
                |r| r.get(0),
            )
            .optional()?;
        return Ok(match hit {
            Some(id) => DedupIdentity::Existing {
                // 幂等键命中（内容无关）：回传已有 id。
                id: Some(id),
            },
            None => DedupIdentity::New { dedup_hash },
        });
    }
    // 无键：回退确定性内容哈希兜底；命中回传 id:None（冻结契约，不回归）。
    let hit: Option<String> = conn
        .query_row(
            "SELECT id FROM transactions \
             WHERE dedup_hash=?1 AND is_deleted=0 ORDER BY created_at LIMIT 1",
            rusqlite::params![dedup_hash],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match hit {
        Some(_) => DedupIdentity::Existing { id: None },
        None => DedupIdentity::New { dedup_hash },
    })
}

/// 记录导入批次汇总日志（ADR-0009 决策 #5 / issue #45）。
///
/// 总耗时用调用方在批次开始时记下的手动 `Instant` 计算；`total` 为批次提交的
/// 交易条数，`failed` 为失败条数，`committed` 区分成功提交与回滚（错误路径同样
/// 记录一条 `info!`，保证回滚后汇总行仍出现）。
fn log_batch_summary(started: Instant, total: usize, failed: usize, committed: bool) {
    let msg = if committed {
        "导入批次完成"
    } else {
        "导入批次回滚"
    };
    tracing::info!(
        total,
        failed,
        elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
        msg
    );
}
