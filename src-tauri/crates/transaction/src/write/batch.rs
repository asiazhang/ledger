//! 批量写入编排（写路径，issue #53 / #63）：`TransactionBatch::run` 单入口。
//!
//! 职责：批次事务、逐条 INSERT + 回写 `dedup_hash`/`idempotency_key`、批次汇总日志
//! 与 `dedup` 注入的去重身份判定（幂等键优先 / 内容哈希兜底，ADR-0010 冻结契约）。
//! 不变量：单笔落库不在此重演（`crate::write::protocol::create`）；逐条响应形状与
//! 事务/去重语义不变；调用方须经 `db::write`（ADR-0032 置脏单点），本模块对备份域
//! 零感知；命中去重的行不进创建入口，幂等重放不产生碎商户。ADR 指针：ADR-0009
//! 决策 #5 / ADR-0010 / ADR-0032 / ADR-0044 决策 4；陷阱：`run` 同时服务 HTTP 批量导入（`dedup=true`）与 IPC 批量创建（`dedup=false`）。

use std::time::Instant;

use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::amount::TransactionKind;
use crate::model::{CreateTransactionResult, TransactionInput};
use crate::write::protocol::create as create_transaction_internal;
use ledger_infra::db::tx_scope::hold_transaction;
use ledger_infra::error::{AppError, ErrClass, Result};
use ledger_infra::signals::WriteEvidence;

/// 批量写入结果：逐条创建结果（与 HTTP/IPC 响应形状一致）+ 聚合证据。
/// `evidence` = 「本批任一实际落库的行即建了商户」（[`WriteEvidence::MerchantCreated`]）
/// ——去重命中行不进创建入口不携证据；壳层据此经 `signals_for` 判定发参考失效信号。
#[derive(Debug)]
pub struct BatchOutcome {
    /// 逐条创建结果（顺序与入参一致）。
    pub results: Vec<CreateTransactionResult>,
    /// 聚合证据：本批是否任一行即建商户。
    pub evidence: WriteEvidence,
}

/// 批量写入交易的深度模块：向调用方暴露一个稳定入口 `run`，承载全部批量编排语义。
#[derive(Debug, Clone, Copy, Default)]
pub struct TransactionBatch;

impl TransactionBatch {
    /// 批量写入一笔或多笔交易（事务 / 去重 / 汇总日志 / 索引消费）。
    ///
    /// 去重身份判定（T1/issue #62 的 `dedup_identity`，幂等键优先 / 内容哈希兜底）只在
    /// `dedup=true` 时生效，`dedup=false` 直接落库；单条校验失败（Invalid 类：既有
    /// `AppError::Invalid` 与码化 `AppError::Coded`（class=Invalid））
    /// 返回 `success:false`+`error` 且不影响同批其他交易，提交失败则整批回滚并在回滚路径
    /// 打批次汇总日志。置脏与写时到期检查不在本函数：调用方经写入口（[`ledger_infra::db::write`]）
    /// 调用时在提交点单点承接，回滚不置脏同由写入口保证（issue #245）。
    pub fn run(
        conn: &Connection,
        inputs: Vec<TransactionInput>,
        dedup: bool,
    ) -> Result<BatchOutcome> {
        let started = Instant::now();
        let total = inputs.len();
        // 失败条数：累计到批次汇总日志（成功路径=无效行数；回滚路径含触发回滚的那条）。
        let mut failed = 0usize;
        // 批次外层事务壳（issue #310 / #1014）：整批原子——单行 Invalid 跳过，
        // 非 Invalid 失败直接返回错误、由 `hold_transaction` 整体回滚。无条件自持
        // 原语归基础设施 `db::tx_scope`（#1003 grilling 定案 3/4）；`PRAGMA optimize`
        // 与汇总日志留批次层、包在本调用点外（#1003 定案 8）。
        let outcome = hold_transaction(conn, || {
            let mut results = Vec::with_capacity(total);
            // 聚合证据：任一实际落库的行即建商户即为真（ADR-0044 决策 4，issue #331）。
            let mut any_merchant_created = false;
            for input in inputs {
                // 客户端幂等键：带键时作为去重身份（内容无关），无键时 None 走内容哈希兜底。
                let idempotency_key = input.idempotency_key.clone();
                // 去重身份判定用 T1/issue #62 的 `dedup_identity`：带键按键查（走部分唯一索引）、
                // 无键回退内容哈希（走部分索引）；ADR-0010 的契约编码在 `DedupIdentity` 类型里，不散在 if 分支。
                // `New` 携带内容哈希供落库回写 `dedup_hash` 列，避免重复计算。
                // 单条写入（含 buy/sell 的持仓副作用路径）由行为层创建编排入口
                // `behavior::create`（issue #228 / ADR-0033）承担：本函数持有外层批次事务，
                // 入口以嵌套模式加入（失败直接返回错误、回滚归本层），其交易行落库
                // 已收口到 `crate::write::writer` 接缝（issue #60）：列映射与审计字段统一由
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
                    Ok(write) => {
                        // 聚合复用证据自身的形状判定（与映射单点同一份，不自写 matches!）。
                        any_merchant_created |= write.evidence.merchant_created();
                        if let Err(e) = conn.execute(
                            "UPDATE transactions SET dedup_hash=?1, idempotency_key=?2 WHERE id=?3",
                            rusqlite::params![dedup_hash, idempotency_key, write.id],
                        ) {
                            failed += 1;
                            return Err(e.into());
                        }
                        results.push(CreateTransactionResult {
                            success: true,
                            duplicate: false,
                            id: Some(write.id),
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
                    // 码化 Invalid（issue #342 二期）：行为层校验失败码化后同归「单行失败」，
                    // 不改变「单行校验失败不回滚整批」的编排语义。
                    Err(AppError::Coded {
                        class: ErrClass::Invalid,
                        message,
                        ..
                    }) => {
                        failed += 1;
                        results.push(CreateTransactionResult {
                            success: false,
                            duplicate: false,
                            id: None,
                            error: Some(message),
                        });
                    }
                    Err(e) => {
                        failed += 1;
                        return Err(e);
                    }
                }
            }
            Ok(BatchOutcome {
                results,
                evidence: WriteEvidence::MerchantCreated(any_merchant_created),
            })
        });
        let outcome = match outcome {
            Ok(outcome) => outcome,
            // 中途失败：`hold_transaction` 已尽力整体回滚，批次层记汇总日志后上抛
            // （回滚路径含触发回滚的那条失败，与提交成功路径的无效行数口径区分）。
            Err(e) => {
                log_batch_summary(started, total, failed, false);
                return Err(e);
            }
        };
        // 批量提交后重跑统计（issue #490）：批量写入是唯一的批量落库入口，
        // 每批提交后执行 `PRAGMA optimize`——按表行数变化启发式增量 ANALYZE，
        // 统计无需刷新时零开销、只刷需要的表，批量大小不设门槛；新装库迁移期
        // 空表统计在此随数据积累逐步收敛。统计过期会误导 planner join 顺序与
        // 索引选择（时点持仓等基准依赖统计假设），失败仅记日志不上抛——统计
        // 刷新不影响已提交的业务写。
        if let Err(e) = conn.execute("PRAGMA optimize", []) {
            tracing::warn!(error = %e, "批量导入后 PRAGMA optimize 失败（忽略）");
        }
        // 汇总行在 COMMIT 后立即打一条（ADR-0009 决策 #5 / issue #45）：
        // 数据已提交，批次应有一条可观测的汇总行。置脏已随迁移收口写入口
        // （issue #245）：调用方的 db.write 闭包在此处返回 Ok 后于提交点触发。
        log_batch_summary(started, total, failed, true);
        Ok(outcome)
    }
}

/// 计算导入去重哈希：`sha256("date|kind|amount_cents|currency_code|account_id|to_account_id")`，
/// 携带出资账户时追加 `|funding_account_id`（issue #939 / ADR-0096 决策 8）；
/// 基金转换（ADR-0099）追加四腿字段（转入标的/转入份额/两侧确认金额）——多腿
/// 转换单由提交方拆成多条 convert 记录（每腿一条），仅日期/账户/金额占位相同的
/// 两腿必须判为不同内容，否则第二腿被内容哈希误判重复而静默丢弃。
/// 现金分红（ADR-0109 / issue #1078）追加归属标的：同一天同一账户同额的两笔分红
/// 分属不同标的时必须判为不同内容，否则后一笔被误判重复而静默丢弃——AI 导入是
/// 分红的主写入面，同日多只基金各自分红是常见形态。
///
/// `to_account_id` 缺省拼空串；刻意排除 note/category（AI 生成文本非确定性，会让哈希漂移）。
/// 出资账户与转换腿字段仅在携带时追加：其余输入与旧公式逐字节同输入——历史行
/// `dedup_hash` 列（旧公式产物）对新公式仍命中，重导去重行为不变。
/// **标的仅在 dividend 追加**（同类「仅携带时追加」纪律）：buy/sell 的历史行以旧
/// 公式落 `dedup_hash`，为其补同一字段会让历史行对新公式失配、重导产生重复，
/// 属 ADR-0010 冻结契约的破坏，故不做。
pub fn compute_dedup_hash(input: &TransactionInput) -> String {
    let to_account_id = input.to_account_id.as_deref().unwrap_or("");
    let mut payload = format!(
        "{}|{}|{}|{}|{}|{}",
        input.date,
        input.kind.as_str(),
        input.amount_cents,
        input.currency_code,
        input.account_id,
        to_account_id
    );
    if let Some(funding) = input.funding_account_id.as_deref() {
        payload.push('|');
        payload.push_str(funding);
    }
    if input.kind == TransactionKind::Convert {
        payload.push('|');
        payload.push_str(input.to_instrument_id.as_deref().unwrap_or(""));
        payload.push('|');
        payload.push_str(&input.to_quantity.unwrap_or(0.0).to_string());
        payload.push('|');
        payload.push_str(&input.out_amount_cents.unwrap_or(0).to_string());
        payload.push('|');
        payload.push_str(&input.in_amount_cents.unwrap_or(0).to_string());
    }
    if input.kind == TransactionKind::Dividend {
        payload.push('|');
        payload.push_str(input.instrument_id.as_deref().unwrap_or(""));
    }
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
/// 两条查询路径各有索引依据（索引定义以迁移为唯一事实来源，V001/V007）：
/// 带客户端幂等键时按键查——内容无关、命中优先走部分唯一索引
/// `idx_transactions_idempotency_key`；无键时回退确定性内容哈希
/// （`compute_dedup_hash`，排除 note/category），走部分索引
/// `idx_transactions_dedup_hash`（V001 就地新增，仅全新安装携带；
/// 旧 schema 存量库兜底查询维持全表扫描，见 V001 文件头就地修改注记）。
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
    // 无键：回退确定性内容哈希兜底（走部分索引 `idx_transactions_dedup_hash`）；命中回传 id:None（冻结契约，不回归）。
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

#[cfg(test)]
mod tests;
