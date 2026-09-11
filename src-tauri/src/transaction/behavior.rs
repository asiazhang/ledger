//! 交易类型行为层（issue #72 / spec #69 候选 2）：按 kind 收敛分派。
//!
//! 对外暴露**三个编排入口** [`create`] / [`update`] / [`delete`]（issue #228 / #229 /
//! ADR-0033：连接 + 输入进，返回终态或错误）——写入时序、事务边界与守卫文案全部
//! 内化为实现细节，调用方只传连接与输入、处理报错。
//!
//! **写入协议（create / update，issue #1004 / ADR-0105）**：写入 grammar——守卫 →
//! 回退（修改路径）→ 计划装配 → 落库 → 应用副作用 → op 产出——按入口各自收进
//! 单正文协议函数 [`create_protocol`] / [`update_protocol`]（两者顺序真不同，不强行
//! 合一），本地写入与同步重放（[`replay_command`]，见多端同步域 Replay）作为
//! **Local / Replay 两形态**，经形态闭集参数（[`WriteForm`] 及其载荷载体
//! [`CreateForm`] / [`UpdateForm`]）吸收四笔刻意差异——守卫与顺序改动一处生效，
//! 两形态对称性由编译期单正文构造保证：
//! - **计划装配来源**：Local 按输入重折算（归一化 + 本位币折算 + 商户即建证据）；
//!   Replay 按命令携带行装配（投资域重放形态计划，不重折算，源端折算 ADR-0091
//!   决策 3）；
//! - **id 来源**：落库时本端生成 vs 随命令携带（两端收敛同一行）；
//! - **op 发射**：本地逐笔留痕 vs 重放不产本地 op（CONTEXT-sync 契约④）；
//! - **存活校验**：Replay 独有（缘由见 [`WriteForm::Replay`] 与协议注记——刻意
//!   不对称，ADR-0105 决策 4）。
//!
//! kind 守卫（dividend 显式拒绝；split 的创建 / 就地修改 / 删除在 Local 形态放行、
//! Replay 形态拒绝挂起，与进/出 split 的 kind 变更禁止——ADR-0106；进/出 convert 的
//! kind 变更禁止，ADR-0099 决策 5）单点进协议本体，两形态同码同文案；穷尽 match
//! 兜底臂沿 ADR-0060 现行写法（同码错误表达不可达态，不引入 panic 构造）。
//!
//! **嵌套感知事务（「保证处于事务中」，ADR-0033 决策 #2）**：create / update 的
//! 事务落点由协议本体自持（ADR-0105 决策 5 的落点细化）——[`ensure_transaction`]
//! 检测连接事务状态，autocommit 则自持 BEGIN/COMMIT/ROLLBACK（中途失败整体回滚，
//! 无中间态泄漏）；已在事务中则加入外层、失败直接返回错误，回滚归外层持有者
//! （批量导入的批次事务与余额调整的外层事务壳是嵌套模式的合法使用者，issue #310；
//! 同步引擎的重放外层事务同样嵌套加入、无害，「命令执行 + op 落日志 + 位点推进」
//! 同事务原子性不变）。提交点副作用（备份置脏，ADR-0032，issue #245 起含嵌套
//! 模式）统一归连接层写入口：调用方的 `db.write` 闭包返回 Ok 且已提交时单点触发
//! ——自持事务与批量嵌套同一形态，本模块对备份域零感知。delete 维持入口持有现状
//! （ADR-0105 决策 1：软删编排不重塑，作「双形态单正文」的参照实现）。
//!
//! **守卫文案收口投资域（ADR-0033 决策 #4 修订，issue #1020）**：buy 部分卖出 /
//! 转换链 / 转换转入份额被后续卖出的拒绝文案与错误码随守卫知识归投资域 `unwind`
//! 模块，按 kind × 修改/删除模式单点选择；行为层不再持守卫文案常量。回退分派直接
//! 委托 [`investment::revert`]（其 match 已覆盖全部 kind，普通 kind 为 no-op），
//! 行为层不再另设 revert 转发层。删除入口不设部分卖出守卫（issue #940 / ADR-0097）：
//! sell 删除回补持仓、buy 删除级联软删其在用 sell——「已有部分卖出的买入禁删」随之退场。
//!
//! 分派是薄而穷尽的 `match`（不引入 trait 注册表，避免过度设计）：
//! 普通 kind（income/expense/transfer/refund）经 Writer 接缝归一化；buy/sell/convert/split
//! 委托投资域（`investment` 域入口的 prepare/apply/revert，正向分派保留；Replay 形态经
//! `investment` 的 replay_plan / replay_convert_plan 装配）；`dividend` 协议守卫单点
//! 显式「暂不支持」拒绝（split 已随 ADR-0106 激活，其 Replay 形态暂拒绝挂起、
//! #1053 收编）。拒绝均不落库。
//!
//! **结果证据外传（ADR-0044 决策 4，issue #331）**：计划装配在入参带 `merchant_name`
//! 且未命中时即建商户（写 `merchants` 表，第四张参考表），「是否即建」作为
//! [`WriteEvidence::MerchantCreated`] 随创建/修改入口的自然返回值外传——壳层据此经
//! `signals_for` 判定发 `ledger:changed`（仅命中复用为零信号）；批量导入在批次层聚合
//! 「任一行即建」。Replay 形态装配信任源端归一化行、不解析商户名，证据恒假。
//!
//! 依赖方向：命令层（transactions → investment → 无反向）。协议保证行写入与
//! lot/匹配副作用同处一个事务（协议自持，或加入调用方外层事务）。

use rusqlite::Connection;
use rusqlite::OptionalExtension;

use super::command::{
    ConvertCommandFields, InvestmentCommandFields, TransactionCommand, record_local,
};
use super::model::{NormalizedTransaction, TransactionInput};
use crate::accounts::balance::{affected_accounts, refresh_account_balances};
use crate::db::now_iso;
use crate::db::tx_scope::ensure_transaction;
use crate::error::{AppError, Result};
use crate::investment;
use crate::signals::WriteEvidence;
use crate::sync_engine::device_id;

use super::amount::TransactionKind;
use super::writer;

pub use create as create_transaction;
pub use create as create_transaction_internal;
pub use delete as delete_transaction;
pub use delete as delete_transaction_internal;
pub use update as update_transaction;
pub use update as update_transaction_internal;

/// 进/出 convert 的 kind 变更拒绝文案与错误码（issue #979 / ADR-0099 决策 5）：
/// 修改协议守卫段单点持有，Local / Replay 两形态同码同文案（同一入口同一文案，
/// ADR-0033 决策 #4）。
const CONVERT_KIND_CHANGE_FORBIDDEN_CODE: &str = "trade.convert-kind-change-forbidden";
const CONVERT_KIND_CHANGE_FORBIDDEN: &str =
    "不可将交易类型改为或改出「转换」：转换的纠错只有「删除后重建」一条路";

/// 进/出 split 的 kind 变更拒绝文案与错误码（ADR-0106 决策 5，沿用 convert 先例）：
/// 与 convert 守卫同段单点，Local / Replay 两形态同码同文案。修改/删除 split 行的
/// 就地修改与删除已收编（#1051：全字段替换 + 逐批次精确回补），仅重放形态仍显式
/// 拒绝挂起（重放端本地重述重建归 #1053，与创建协议同一口径）。
const SPLIT_KIND_CHANGE_FORBIDDEN_CODE: &str = "trade.split-kind-change-forbidden";
const SPLIT_KIND_CHANGE_FORBIDDEN: &str =
    "不可将交易类型改为或改出「份额调整」：份额调整的纠错只有「删除后重建」一条路";

// ---------------------------------------------------------------------------
// 写入形态闭集（issue #1004 / ADR-0105）
// ---------------------------------------------------------------------------

/// 写入形态闭集：本机写入（Local）与重放执行（Replay）的二值区分，本文件
/// 「Local / Replay」的唯一真源——由 delete 路径既有 `OpEmission` 改名推广而来，
/// create / update 协议与 delete 路径共用同一枚举（不留第二份同义区分）。
///
/// 四笔形态差异（计划装配来源、id 来源、op 发射、存活校验）在协议分歧点 match
/// 本枚举；create / update 两形态载荷不同构，经 [`CreateForm`] / [`UpdateForm`]
/// 承载（变体与本枚举一一对应），delete 两形态载荷同构（实体 id），直接消费。
///
/// 注意与投资域 `unwind::Mode`（Update / Delete）同名不同义：那是「修改 vs 删除」
/// 的入口形态轴（ADR-0105 决策 6 防混淆注记），本枚举是「本地 vs 重放」的写入
/// 形态轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteForm {
    /// 本机写入：id 本端生成、装配按输入重折算、op 逐笔留痕、无账户存活校验
    /// （**刻意不对称**——本地账户引用来自在用字典选取，往软删账户落账的口子由
    /// 前端守住，后端补守卫属用户可见行为变化，口子另立缺陷票处置，不夹带；
    /// ADR-0105 决策 4）。
    Local,
    /// 重放执行：id 随命令携带（两端收敛同一行）、装配按命令携带行（不重折算，
    /// 源端折算 ADR-0091 决策 3）、不产本地 op（引擎在重放事务内落日志）、账户
    /// 引用存活校验（ParkedOp「不自动复活已删数据」契约的组成部分，issue #856）。
    Replay,
}

/// 创建协议的形态参数（[`WriteForm`] 闭集的载荷载体）：两形态装配输入不同构——
/// Local 携本地输入（装配时重折算），Replay 携重放命令部件（实体 id 与归一化行
/// 随命令携带）。变体与 [`WriteForm`] 一一对应。
#[derive(Debug)]
enum CreateForm<'a> {
    Local(&'a TransactionInput),
    Replay {
        id: &'a str,
        row: &'a NormalizedTransaction,
        investment: Option<&'a InvestmentCommandFields>,
        convert: Option<&'a ConvertCommandFields>,
    },
}

impl CreateForm<'_> {
    /// 写入目标 kind（协议守卫段读取）：Local 取输入，Replay 取命令携带行。
    fn kind(&self) -> TransactionKind {
        match self {
            CreateForm::Local(input) => input.kind,
            CreateForm::Replay { row, .. } => row.kind,
        }
    }
}

/// 修改协议的形态参数（[`WriteForm`] 闭集的载荷载体）：目标 id 两形态同构
/// （都按 id 寻址既有行），装配输入不同构——同 [`CreateForm`]。
#[derive(Debug)]
enum UpdateForm<'a> {
    Local(&'a TransactionInput),
    Replay {
        row: &'a NormalizedTransaction,
        investment: Option<&'a InvestmentCommandFields>,
        convert: Option<&'a ConvertCommandFields>,
    },
}

impl UpdateForm<'_> {
    /// 写入目标 kind（协议守卫段读取）：Local 取输入，Replay 取命令携带行。
    fn kind(&self) -> TransactionKind {
        match self {
            UpdateForm::Local(input) => input.kind,
            UpdateForm::Replay { row, .. } => row.kind,
        }
    }
}

// ---------------------------------------------------------------------------
// 计划与终态
// ---------------------------------------------------------------------------

/// 计划：归一化后的交易行 + kind 特有副作用数据（不落库）。
enum Plan {
    /// 普通 kind（income/expense/transfer/refund）：无副作用。
    Common(writer::NormalizedRow),
    /// 投资 kind（buy/sell/convert）：归一化行与副作用数据留在投资域计划中。
    Investment(investment::Plan),
}

impl Plan {
    /// 归一化交易行（供 [`writer::insert_row`] / [`writer::update_row`] 落库）。
    fn normalized_row(&self) -> Result<writer::NormalizedRow> {
        match self {
            Plan::Common(row) => Ok(row.clone()),
            Plan::Investment(p) => Ok(writer::NormalizedRow::try_from(p.normalized())?),
        }
    }
}

/// 交易创建的终态与结果证据（ADR-0044 决策 4）：`id` 供调用方回传 / 回写去重身份，
/// `evidence` 随写操作自然外传（非可变状态、不穿透 `db.write` 闭包）——壳层据此经
/// `signals_for` 判定发射。证据恒为 [`WriteEvidence::MerchantCreated`]（交易域唯一的
/// 条件信号），未涉商户的写（buy/sell 等，或直接带 id / 不带商户）证据恒假、零信号。
#[derive(Debug)]
pub struct TransactionWrite {
    /// 新交易 id。
    pub id: String,
    /// 本次写入证据：入参带 `merchant_name` 且未命中即建为真，仅命中复用为假。
    pub evidence: WriteEvidence,
}

// ---------------------------------------------------------------------------
// 编排入口与写入协议
// ---------------------------------------------------------------------------

/// 创建一笔交易（IPC `create_transaction` / 批量导入批次循环 / 余额调整的交易写入，
/// issue #228 / #310 / ADR-0033）。
///
/// 行为层创建编排入口：写入协议（守卫 → 计划装配 → 落库 → 应用副作用 → op 产出，
/// ADR-0105）的 Local 形态薄壳——顺序契约、事务边界（协议本体自持，见
/// [`ensure_transaction`]）与守卫文案全部内化，调用方只传连接与输入、处理报错。
/// 返回值携带终态 id 与「是否即建商户」证据（ADR-0044 决策 4，issue #331）。
///
/// 可见性说明：三个入口函数本体 `pub`（供 [`super`] 以 `*_transaction_internal`
/// 名字公开再导出，模块本身私有故不额外扩大可见面）。
pub fn create(conn: &Connection, input: TransactionInput) -> Result<TransactionWrite> {
    create_protocol(conn, CreateForm::Local(&input))
}

/// 创建写入协议本体（ADR-0105）：守卫 → 计划装配 → 落库 → 应用副作用 → op 产出，
/// 单正文承载 Local / Replay 两形态；「保证处于事务中」由本函数自持（嵌套感知，
/// ADR-0033 决策 2，落点细化为协议本体，ADR-0105 决策 5）。
///
/// 守卫段（分歧点内嵌）：
/// - 参考数据携带准入——仅 Local 按输入判定（issue #188/#361/#582 收口），须先于
///   kind 守卫（准入拒绝优先于「暂不支持」，拒绝行不产生字典碎片）；Replay 信任
///   源端归一化行、不再判定；
/// - kind 守卫单点——dividend 已声明但未实现（MVP）两形态同码同文案显式拒绝、
///   不落库；split 的 Local 形态进入投资域装配（ADR-0106），Replay 形态显式拒绝
///   挂起（#1053 收编）；Replay 的伪造/漂移载荷防御臂先于依赖校验，不误报账户缺失；
/// - 存活校验——仅 Replay（issue #856）：账户引用必须存活，往已删账户记账码化
///   拒绝、引擎挂起待裁决；Local 刻意不补（见 [`WriteForm::Local`]）。
///
/// op 产出为最后一步：写与副作用全部成功后才追加，随同一事务提交/回滚（失败不
/// 残留 op）。Replay 形态不产本地 op；其返回值 id 即命令携带值、证据恒假（装配
/// 不即建商户），仅为统一签名保留。
fn create_protocol(conn: &Connection, source: CreateForm<'_>) -> Result<TransactionWrite> {
    ensure_transaction(conn, || {
        // ── 守卫段 ──
        if let CreateForm::Local(input) = &source {
            guard_reference_admission(input)?;
        }
        let kind = source.kind();
        if kind == TransactionKind::Dividend {
            return Err(kind_unsupported(kind));
        }
        // split：本机创建放行（ADR-0106 决策 10，AI / 契约是唯一写入面）；重放形态
        // 本票暂不支持（重放端本地重述重建归 #1053），显式拒绝由引擎挂起承接。
        if kind == TransactionKind::Split && matches!(source, CreateForm::Replay { .. }) {
            return Err(kind_unsupported(kind));
        }
        if let CreateForm::Replay { row, .. } = &source {
            writer::validate_accounts_alive(
                conn,
                &row.account_id,
                row.to_account_id.as_deref(),
                row.funding_account_id.as_deref(),
            )?;
        }
        // ── 计划装配（分歧点①：Local 按输入重折算 / Replay 按命令携带行）──
        let (plan, merchant_created) = match &source {
            CreateForm::Local(input) => plan(conn, input, None)?,
            CreateForm::Replay {
                row,
                investment,
                convert,
                ..
            } => (
                replay_assembly(conn, row, *investment, *convert)?,
                // 重放形态不解析商户名、不即建商户，证据恒假。
                false,
            ),
        };
        let row = plan.normalized_row()?;
        // ── 落库（分歧点②：id 来源——本端生成 vs 随命令携带）──
        // 搜索（V018 两段式，issue #492）：Writer 接缝同写维护 note_pinyin 派生列，
        // 交易立即可搜（存量积压由读路径惰性回填兜底）。
        let id = match &source {
            CreateForm::Local(_) => writer::insert_row(conn, &row)?,
            CreateForm::Replay { id, .. } => {
                writer::insert_row_with_id(conn, id, &row)?;
                id.to_string()
            }
        };
        // ── 应用副作用 ──
        apply(conn, &id, &plan)?;
        // ── op 产出（分歧点③：本地逐笔留痕 / 重放不产本地 op）──
        if let CreateForm::Local(_) = &source {
            record_local(conn, create_command(&id, &plan))?;
        }
        Ok(TransactionWrite {
            id,
            evidence: WriteEvidence::MerchantCreated(merchant_created),
        })
    })
}

/// 按 `id` 全字段替换一笔交易（IPC `update_transaction` / HTTP
/// `PUT /api/v1/transactions/{id}`，issue #229 / ADR-0033）。
///
/// 行为层修改编排入口：写入修改协议（守卫 → 回退 → 计划装配 → 落库 → 应用副作用
/// → op 产出，ADR-0105）的 Local 形态薄壳——持仓守卫语义与文案归投资域 `unwind`
/// （issue #1020），本入口只承接分派。
///
/// 幂等键（`idempotency_key`）与内容哈希（`dedup_hash`）不作为
/// 可编辑字段——修改不重算去重身份，故修改后重跑同批导入（带幂等键）仍按同键去重、不产生重复。
/// 不存在或已软删除的 id 返回码化 NotFound（`transaction.not-found`）。
/// 返回「是否即建商户」证据（[`WriteEvidence::MerchantCreated`]，issue #331）。
pub fn update(conn: &Connection, id: &str, input: TransactionInput) -> Result<WriteEvidence> {
    update_protocol(conn, id, UpdateForm::Local(&input))
}

/// 修改写入协议本体（ADR-0105）：守卫 → 回退 → 计划装配 → 落库 → 应用副作用 →
/// op 产出，单正文承载 Local / Replay 两形态；事务落点同创建协议（本体自持）。
///
/// 与创建协议的顺序差异（两者不强行合一的缘由）：修改多「旧行并集读取 → convert
/// kind 变更守卫 → 回退」三步。
///
/// 守卫段：
/// - 旧行并集读取（ADR-0105 决策 8）——kind + 商户 + 保单一次读出：本地用于
///   「保持历史引用」判定（提交值与原值相同则跳过在用校验，已软删商户/保单的
///   历史交易仍可修改其他字段），重放仅用 kind（多读两列无可观察影响）；协议内
///   单次读取，不存在或已删除返回码化 NotFound（两形态同款）。读取在事务内
///   （协议已保证处于事务中，消除读取与 BEGIN 之间的窗口，ADR-0033 决策 #5）。
/// - convert kind 变更守卫单点（ADR-0099 决策 5）——从 convert 出、或改为 convert
///   为 PUT 专属码化拒绝：转换的纠错只有「软删 + 重建」一条窄路，重放端不得由
///   伪造 op 绕过；就地修改转换走同一协议（convert → convert 由
///   [`investment::revert`] 的转换清理承载）。
/// - 参考数据携带准入——仅 Local，先于 kind 守卫（同创建协议）。
/// - kind 守卫单点——dividend「暂不支持」拒绝；进/出 split 的 kind 变更拒绝与
///   split 就地修改的 Replay 形态拒绝挂起（Local 形态全字段替换放行，#1051）。
///
/// 新值守卫全部先于回退：对非法新值先行拒绝、不做任何突变（fail fast，与重放
/// 形态既有顺序一致）。回退按旧 kind 执行（跨 kind 修改避免孤儿持仓）；存活校验
/// 仅 Replay（同创建协议，置于回退后、装配前——与投资域装配的依赖在位校验同族）。
fn update_protocol(conn: &Connection, id: &str, source: UpdateForm<'_>) -> Result<WriteEvidence> {
    ensure_transaction(conn, || {
        // ── 旧行并集读取（ADR-0105 决策 8）──
        let (old_kind, old_merchant_id, old_policy_id): (
            TransactionKind,
            Option<String>,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT kind, merchant_id, policy_id FROM transactions WHERE id=?1 AND is_deleted=0",
                rusqlite::params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or_else(|| {
                AppError::codedp_not_found(
                    "transaction.not-found",
                    format!("交易不存在: {id}"),
                    &[id],
                )
            })?;
        // ── 守卫段 ──
        let new_kind = source.kind();
        if (old_kind == TransactionKind::Convert) != (new_kind == TransactionKind::Convert) {
            return Err(AppError::coded(
                CONVERT_KIND_CHANGE_FORBIDDEN_CODE,
                CONVERT_KIND_CHANGE_FORBIDDEN,
            ));
        }
        // 进/出 split 的 kind 变更一律拒绝（ADR-0106 决策 5，与 convert 同规）。
        if (old_kind == TransactionKind::Split) != (new_kind == TransactionKind::Split) {
            return Err(AppError::coded(
                SPLIT_KIND_CHANGE_FORBIDDEN_CODE,
                SPLIT_KIND_CHANGE_FORBIDDEN,
            ));
        }
        // split 行就地修改（全字段替换）：Local 形态经「回退（逐批次精确回补）→
        // 重装（按新输入重述在用批次）→ 落库 → 重述副作用」实现（ADR-0106 决策 3/5，
        // #1051）；重放形态本票仍显式拒绝挂起（重放端本地重述重建归 #1053，与创建
        // 协议同一口径——否则会先回退再在装配兜底臂挂起，回退无谓地发生）。
        if new_kind == TransactionKind::Split && matches!(&source, UpdateForm::Replay { .. }) {
            return Err(kind_unsupported(new_kind));
        }
        if let UpdateForm::Local(input) = &source {
            guard_reference_admission(input)?;
        }
        if new_kind == TransactionKind::Dividend {
            return Err(kind_unsupported(new_kind));
        }
        // ── 回退（修改路径步骤）：先按旧 kind 回退持仓/卖出关联副作用，再按新
        // kind 装配落库；buy/convert 守卫（已有部分卖出 / 份额已被后续转换消耗）
        // 语义与措辞归投资域 unwind 模块。 ──
        investment::revert(conn, id, old_kind)?;
        // 存活校验（分歧点：仅 Replay，issue #856 / #935）：修改不得把交易改挂到
        // 已删账户上（含出资端），码化拒绝由引擎挂起待裁决。
        if let UpdateForm::Replay { row, .. } = &source {
            writer::validate_accounts_alive(
                conn,
                &row.account_id,
                row.to_account_id.as_deref(),
                row.funding_account_id.as_deref(),
            )?;
        }
        // ── 计划装配（分歧点①）──
        let (plan, merchant_created) = match &source {
            UpdateForm::Local(input) => plan_with_existing_refs(
                conn,
                input,
                old_merchant_id.as_deref(),
                old_policy_id.as_deref(),
                Some(id),
            )?,
            UpdateForm::Replay {
                row,
                investment,
                convert,
            } => (replay_assembly(conn, row, *investment, *convert)?, false),
        };
        let row = plan.normalized_row()?;
        // ── 落库（目标 id 两形态同构：id 即寻址既有行，无 id 来源分歧）──
        writer::update_row(conn, id, &row)?;
        // ── 应用副作用 ──
        apply(conn, id, &plan)?;
        // ── op 产出（分歧点③）：修改成功 → 新行（含源端折算）随 update op 追加，
        // 随同一事务提交/回滚；Replay 不产本地 op。 ──
        if let UpdateForm::Local(_) = &source {
            record_local(conn, update_command(id, &plan))?;
        }
        Ok(WriteEvidence::MerchantCreated(merchant_created))
    })
}

// ---------------------------------------------------------------------------
// 删除编排（参照实现：双形态单正文，语法不重塑，ADR-0105 决策 1）
// ---------------------------------------------------------------------------

/// 删除交易（软删除 `is_deleted=1`；IPC `delete_transaction` / HTTP
/// `DELETE /api/v1/transactions/{id}`，issue #229 / ADR-0033）。
///
/// 行为层删除编排入口：持仓副作用回退与软删 UPDATE 同处一个事务——回退成功后
/// 软删 UPDATE 中途失败整体回滚，不再出现「持仓已删而交易仍在」的中间态
/// （删除路径事务缺口修复，ADR-0033 决策 #3）。delete 维持入口持有事务的现状
/// （ADR-0105 决策 1/5）。
///
/// 持仓副作用回退按 kind 分派（issue #940 / ADR-0097，issue #979 / ADR-0099）：
/// - sell：回补其扣减的持仓并清空卖出关联——删除即撤销其全部持仓影响，
///   不再遗留幽灵占用（旧版「sell 删除不回补」是把买入永久锁死的根源）；
/// - buy：**级联**——其持仓批次的在用 sell 逐笔回退持仓副作用并随之软删
///   （各自 delete op 与余额刷新），「已有部分卖出的买入禁删」守卫退场；
/// - convert：**级联**——转出腿逐批次精确回补、转入批次的在用 sell 与 buy 同规
///   级联，再整批清理转入批次与转换明细行（ADR-0099 决策 5）；
/// - 其余 kind 无持仓副作用，直接软删。
///
/// 不存在的 id 返回码化 NotFound（HTTP 侧映射 404）。IPC 与 HTTP 端点共用本函数。
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        delete_within_transaction(conn, id, WriteForm::Local)
    })
}

/// 删除协议本体：`release_for_delete（sell 回补 / buy 与 convert 级联）→ 级联软删
/// 在用 sell → 主行软删`（无事务语义，delete 入口持有；重放经
/// [`replay_command`] 在引擎外层事务内进入）。
fn delete_within_transaction(conn: &Connection, id: &str, form: WriteForm) -> Result<()> {
    let (kind,): (TransactionKind,) = conn
        .query_row(
            "SELECT kind FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| Ok((r.get(0)?,)),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::codedp_not_found("transaction.not-found", format!("交易不存在: {id}"), &[id])
        })?;

    // 持仓副作用回退（issue #940 / ADR-0097 / issue #979 / ADR-0099）：sell 回补
    // 持仓扣减；buy / convert 消费其持仓批次的在用 sell（逐笔回退）并整批清理批次
    // 与匹配（convert 另回补转出腿消耗）；本行批次已被在用后续转换消耗时，以删除
    // 入口守卫拒绝。split 行按 `security_lot_adjustments` 逐批次精确回补（ADR-0106
    // 决策 3/5，#1051）；重放形态仍显式拒绝挂起（重放端本地重述重建归 #1053）。
    let cascaded_sell_ids = match kind {
        TransactionKind::Buy
        | TransactionKind::Sell
        | TransactionKind::Convert
        | TransactionKind::Split => {
            if kind == TransactionKind::Split && form == WriteForm::Replay {
                return Err(kind_unsupported(kind));
            }
            investment::release_for_delete(conn, id, kind)?
        }
        _ => Vec::new(),
    };
    // 级联软删：被消化的在用 sell 逐笔软删（各自余额刷新与 delete op），先于主行
    // 落 op——重放端按 op 顺序先删 sell 再删 buy，级联与显式 op 不打架。
    for sell_id in &cascaded_sell_ids {
        soft_delete_transaction_row(conn, sell_id, form)?;
    }
    // 搜索（V018 两段式，issue #492）：候选流带 is_deleted 口径过滤，软删即刻
    // 生效，删除的交易不再可搜（拼音列非本路径维护字段，无需处理）。
    soft_delete_transaction_row(conn, id, form)
}

/// 软删单笔交易行 + 余额刷新 + delete op（删除编排的行级步骤，主删与级联删共用）。
///
/// 行读取（`is_deleted=0` 存在性 + 账户引用对）→ 软删 UPDATE → 受影响账户余额重算
/// （issue #491 / ADR-0067）→ 本地删除时追加 delete op（issue #855 / ADR-0091）。
/// 级联删除的被级联行在**源端**亦各自留痕：同步端按同一 delete 协议逐笔收敛，
/// 无需感知级联语义；重放端不产 op（见 [`WriteForm::Replay`]）。
fn soft_delete_transaction_row(conn: &Connection, id: &str, form: WriteForm) -> Result<()> {
    let (account_id, to_account_id, funding_account_id): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT account_id, to_account_id, funding_account_id FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::codedp_not_found("transaction.not-found", format!("交易不存在: {id}"), &[id])
        })?;
    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    // 余额缓存写路径（issue #491 / ADR-0067）：软删后对原行账户引用三元组（受影响
    // 账户推导，消费余额模块唯一定义，issue #534 / #935——删除恢复出资账户的现金腿）
    // 同事务整体重算。
    let affected = affected_accounts(
        Some((
            account_id.as_str(),
            to_account_id.as_deref(),
            funding_account_id.as_deref(),
        )),
        None,
    );
    refresh_account_balances(conn, &affected)?;
    // op 产出接缝（issue #855 / ADR-0091）：**仅本地删除**追加 delete op（实体 id）；
    // 随同一事务提交/回滚（失败不残留 op）。重放不产 op（见 [`WriteForm`]）。
    if form == WriteForm::Local {
        record_local(conn, TransactionCommand::Delete { id: id.to_string() })?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 守卫与计划装配（协议分歧点①的 Local 臂）
// ---------------------------------------------------------------------------

/// 参考数据携带准入（协议守卫段的 Local 分歧步骤）：商户/分类/保单按 kind 单点
/// 判定准入闭集，准入集外的 kind 携带对应引用即码化拒绝——创建/修改两协议经本
/// 函数共用同一收口，不另设第二份判定。Replay 形态不判定：命令携带行已随源端
/// 归一化落定（信任源端，ADR-0091）。
///
/// 商户携带收口（issue #188 / ADR-0028 + #194 商户名 + #875 / ADR-0092 transfer）：
/// expense / refund / income / transfer 可携带商户（merchant_id 或 merchant_name）；
/// buy / sell / dividend / split 行为层拒绝（schema 层 merchant_id 允许 NULL、不设
/// kind 限制，放开无需再改表）。transfer 放开是纯 kind 判定，**不引入账户类型条件**
/// ——普通转账与借贷转账（receivable/debt 账户派生视角）共用同一收口，借贷关联
/// 语义只是检索与展示指针（见 docs/adr/0092）。refund 携带的商户在
/// [`writer::normalize`] 里被原支出商户覆盖（继承语义），此处不拦截。
///
/// 分类携带收口（issue #582）：expense / income 可携带分类；transfer / buy / sell
/// 行为层拒绝——转账与资本变动没有「花在哪类」的语义，无分类交易因此不进分类
/// 聚合。refund 携带的分类在 [`writer::normalize`] 里被原支出分类覆盖（继承语义），
/// 此处不拦截。dividend / split 携带分类时本拒绝先于「暂不支持」触发（比照商户
/// 收口先例）。
///
/// 保单携带准入（issue #361 / ADR-0051 决策 3）：仅 income / expense 可携带可选
/// 保单引用；transfer / buy / sell / dividend / split 行为层拒绝（保单口径不被
/// 资本变动污染）。refund 不在准入集：保单现金流入记 income 挂单而非 refund
/// （ADR-0051 决策 4），且 refund 继承原支出字段，携带保单无意义，拒绝保持准入集
/// 最小。
fn guard_reference_admission(input: &TransactionInput) -> Result<()> {
    let kind = input.kind;
    if (input.merchant_id.is_some() || input.merchant_name.is_some())
        && !matches!(
            kind,
            TransactionKind::Income
                | TransactionKind::Expense
                | TransactionKind::Refund
                | TransactionKind::Transfer
        )
    {
        return Err(AppError::codedp(
            "transaction.merchant-unsupported",
            format!("交易类型 {kind} 不能携带商户"),
            &[&kind.to_string()],
        ));
    }
    if input.category_id.is_some()
        && !matches!(
            kind,
            TransactionKind::Income | TransactionKind::Expense | TransactionKind::Refund
        )
    {
        return Err(AppError::codedp(
            "transaction.category-unsupported",
            format!("交易类型 {kind} 不能携带分类"),
            &[&kind.to_string()],
        ));
    }
    if input.policy_id.is_some()
        && !matches!(kind, TransactionKind::Income | TransactionKind::Expense)
    {
        return Err(AppError::codedp(
            "transaction.policy-unsupported",
            format!("交易类型 {kind} 不能挂保单"),
            &[&kind.to_string()],
        ));
    }
    Ok(())
}

/// 校验并归一化一笔交易输入为计划（交易行不落库）。计划装配 Local 臂：按输入
/// 重折算（归一化 + 本位币折算）并可即建商户。
///
/// `existing_merchant_id`：修改路径该行当前的商户 id（创建路径传 None）——提交商户
/// 与其相同视为保持历史引用（软删商户的历史交易仍可修改其他字段，见
/// [`writer::normalize`] 的商户校验）；改选其他商户按新选择校验在用。
///
/// 商户名归一化（AI 导入契约，issue #194）：输入带 `merchant_name` 时在此解析为
/// `merchant_id`——精确匹配在用商户名，命中复用、未命中即建。两段式避免碎商户：
/// 先查（[`merchants::find_merchant_by_name`]），未命中则等行内校验全部通过后
/// 再即建（[`merchants::create_merchant_by_name`]）——金额非法等校验失败的行
/// 不会残留无引用的商户行。商户创建与交易落库同处协议自持的事务，中途回滚不残留
/// 碎商户。幂等重放不产生碎商户：批量导入命中去重的行不会走到本函数，同批内首行
/// 即建、后续行按名精确匹配复用。Replay 形态不经过本函数（命令携带行无商户名）。
///
/// 单点分派全部 9 种 kind：通用 kind 经 Writer 接缝 [`writer::normalize`]（金额>0、
/// transfer 目标账户、refund 继承原支出等校验 + 本位币折算）；buy/sell/convert/split
/// 委托投资域 [`investment::prepare`]（投资账户/数量/单价/可卖数量、两标的互异与不跨
/// 账户校验 + 折算；split 另行使零持仓 / 缩股不等式 / 字段级无现金腿守卫）；`dividend`
/// 为穷尽兜底臂——协议守卫单点已先行拒绝，此臂沿 ADR-0060 现行写法以同码错误表达
/// 不可达态（不引入 panic 构造）。
/// 返回 `(计划, 是否即建商户)`：后者即 [`WriteEvidence::MerchantCreated`] 的载荷——
/// 仅「入参带 `merchant_name` 且未命中、行内校验通过后落定即建」为真；命中复用、
/// 直接带 `merchant_id`、refund 继承忽略、不涉商户的 kind 一律为假。
fn plan(
    conn: &Connection,
    input: &TransactionInput,
    existing_merchant_id: Option<&str>,
) -> Result<(Plan, bool)> {
    plan_with_existing_refs(conn, input, existing_merchant_id, None, None)
}

/// [`plan`] 的全量形态：修改路径额外传该行当前的保单 id（保单「保持历史引用」判定，
/// 与商户同款语义）与既有交易 id（split 重述目标以本行落账序为界，ADR-0106 决策 6）。
/// 创建路径两者都传 None。
fn plan_with_existing_refs(
    conn: &Connection,
    input: &TransactionInput,
    existing_merchant_id: Option<&str>,
    existing_policy_id: Option<&str>,
    existing_id: Option<&str>,
) -> Result<(Plan, bool)> {
    let kind = input.kind;
    match kind {
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => {
            // 商户名解析须在 kind 收口之后：非 income/expense/refund/transfer 的行
            // 已在协议准入段拒绝（名字与 id 同时提供属请求错误）。refund 继承原支出
            // 商户（writer::normalize 覆盖）：携带的商户（id 或名字）一律忽略，
            // 不解析、不即建——否则即建商户必成孤儿（issue #194）。
            let (merchant_id, pending_name) = if kind == TransactionKind::Refund {
                (None, None)
            } else {
                match (&input.merchant_name, &input.merchant_id) {
                    (Some(_), Some(_)) => {
                        return Err(AppError::coded(
                            "transaction.merchant-id-and-name-conflict",
                            "merchant_id 与 merchant_name 不可同时提供",
                        ));
                    }
                    (Some(name), None) => {
                        match crate::merchants::find_merchant_by_name(conn, name)? {
                            // 命中复用：以已有 id 参与行内校验。
                            Some(id) => (Some(id), None),
                            // 未命中：先过行内校验，通过后再即建（不残留碎商户）。
                            None => (None, Some(name.to_string())),
                        }
                    }
                    (None, id) => (id.clone(), None),
                }
            };
            let mut norm = writer::normalize(
                conn,
                &writer::Input {
                    kind,
                    amount_cents: input.amount_cents,
                    currency_code: input.currency_code.clone(),
                    account_id: input.account_id.clone(),
                    to_account_id: input.to_account_id.clone(),
                    // 出资账户随输入下传；通用 kind 携带即被 writer::normalize 内的
                    // 准入校验拒绝（issue #935），此处透传不判定。
                    funding_account_id: input.funding_account_id.clone(),
                    category_id: input.category_id.clone(),
                    merchant_id,
                    existing_merchant_id: existing_merchant_id.map(str::to_string),
                    policy_id: input.policy_id.clone(),
                    existing_policy_id: existing_policy_id.map(str::to_string),
                    refund_of_transaction_id: input.refund_of_transaction_id.clone(),
                    note: input.note.clone(),
                    date: input.date.clone(),
                },
            )?;
            // 行内校验全部通过后才即建商户：未命中名字在此落定（失败行不产生碎商户）；
            // 「即建」事实作为证据外传（ADR-0044 决策 4，壳层据此发参考失效信号）。
            let merchant_created = if let Some(name) = pending_name {
                norm.merchant_id = Some(crate::merchants::create_merchant_by_name(conn, &name)?);
                true
            } else {
                false
            };
            Ok((Plan::Common(norm), merchant_created))
        }
        TransactionKind::Buy
        | TransactionKind::Sell
        | TransactionKind::Convert
        | TransactionKind::Split => {
            // 投资 kind 不涉商户（协议准入段已拒绝携带），证据恒假；split 由
            // 投资域 prepare_split 守卫与装配（ADR-0106）。
            Ok((
                Plan::Investment(investment::prepare(conn, kind, input, existing_id)?),
                false,
            ))
        }
        // 协议守卫单点已先行拒绝（dividend 暂不支持；split 的新值 kind 已被上方
        // kind 变更守卫拦截）；穷尽兜底臂以同码错误表达不可达态，不引入 panic
        // 构造（ADR-0060）。
        TransactionKind::Dividend => Err(kind_unsupported(kind)),
    }
}

/// 应用计划的副作用（创建/修改落库后调用）。
fn apply(conn: &Connection, id: &str, plan: &Plan) -> Result<()> {
    match plan {
        Plan::Common(_) => Ok(()),
        Plan::Investment(p) => investment::apply(conn, id, p),
    }
}

// ---------------------------------------------------------------------------
// 同步命令（issue #855 / ADR-0091）：产出侧桥与重放执行形态
// ---------------------------------------------------------------------------

/// 编排计划 → 命令载荷部件（产出侧桥）：归一化行 + 投资 kind 的语义字段与
/// 派生结果（买入每份成本随行，源端折算，ADR-0091 决策 3）；普通 kind 恒 None。
///
/// 转换的语义字段（[`ConvertCommandFields`]）与源端算定的结转成本随同步票
/// （issue #980 / ADR-0099 决策 6）携带：重放端据以本地重建 FIFO 快照并比对
/// 结转成本合计，不依赖源端的批次行 id。
fn command_parts(
    plan: &Plan,
) -> (
    NormalizedTransaction,
    Option<InvestmentCommandFields>,
    Option<ConvertCommandFields>,
) {
    match plan {
        Plan::Common(r) => (NormalizedTransaction::from(r), None, None),
        Plan::Investment(p) => {
            let fields = match p {
                investment::Plan::Buy(b) => Some(InvestmentCommandFields {
                    instrument_id: b.instrument_id.clone(),
                    quantity: b.quantity,
                    price_cents: b.price_cents,
                    fee_cents: b.fee_cents,
                    cost_per_unit_cents: Some(b.cost_per_unit_cents),
                }),
                investment::Plan::Sell(s) => Some(InvestmentCommandFields {
                    instrument_id: s.instrument_id.clone(),
                    quantity: s.quantity,
                    price_cents: s.price_cents,
                    fee_cents: s.fee_cents,
                    cost_per_unit_cents: None,
                }),
                // 份额调整只携带 Δ（复用 quantity，ADR-0106 决策 9）：重述是
                // (批次快照, Δ) 的纯函数，重放端本地重建，不携带重述结果
                //（重放重建归 #1053；在其收编前重放端仍显式拒绝挂起）。
                investment::Plan::Split(s) => Some(InvestmentCommandFields {
                    instrument_id: s.instrument_id.clone(),
                    quantity: s.delta_quantity,
                    price_cents: 0,
                    fee_cents: 0,
                    cost_per_unit_cents: None,
                }),
                // 转换走独立的可选成员（investment 恒 None）：两腿字段与 buy/sell
                // 的语义字段不同构，不塞进同一结构。
                investment::Plan::Convert(_) => None,
            };
            let convert = match p {
                investment::Plan::Convert(c) => Some(ConvertCommandFields {
                    instrument_id: c.instrument_id.clone(),
                    quantity: c.quantity,
                    to_instrument_id: c.to_instrument_id.clone(),
                    to_quantity: c.to_quantity,
                    out_amount_cents: c.out_amount_cents,
                    in_amount_cents: c.in_amount_cents,
                    fee_cents: c.fee_cents,
                    carried_cost_cents: c.carried_cost_cents(),
                }),
                investment::Plan::Buy(_)
                | investment::Plan::Sell(_)
                | investment::Plan::Split(_) => None,
            };
            (p.normalized().clone(), fields, convert)
        }
    }
}

/// 创建命令构造（创建协议 op 产出步专用）。
fn create_command(id: &str, plan: &Plan) -> TransactionCommand {
    let (row, investment, convert) = command_parts(plan);
    TransactionCommand::Create {
        id: id.to_string(),
        row,
        investment,
        convert,
    }
}

/// 修改命令构造（修改协议 op 产出步专用）。
fn update_command(id: &str, plan: &Plan) -> TransactionCommand {
    let (row, investment, convert) = command_parts(plan);
    TransactionCommand::Update {
        id: id.to_string(),
        row,
        investment,
        convert,
    }
}

/// 同步重放入口：执行外来交易命令（issue #855 / ADR-0091）。
///
/// 创建/修改薄壳进各自写入协议的 Replay 形态（守卫、顺序、事务语义与本地写入
/// 同一正文，ADR-0105）；删除直接进删除协议的 Replay 形态（引擎外层事务内嵌套
/// 加入，删除入口不另持事务——见 [`delete`] 的入口持有现状）。重放形态差异由
/// 协议分歧点承载：
/// - **折算随命令携带**（源端折算）：行数据原样落库，不查本地汇率表——重放
///   不依赖本地汇率表状态与设备配置（ADR-0091 决策 3）；
/// - **实体 id 随命令携带**：两端对同一笔交易收敛到同一行；
/// - **不产出 op**：外来 op 由同步引擎（`sync_engine::apply_ops`）在重放事务
///   内落日志，命令执行不得再追加本地 op。
///
/// 投资 kind（buy/sell）与转换（convert）的重放依赖在位校验（标的、投资账户、
/// 可卖数量、结转成本比对）以与本地写入同码的码化错误上抛，由同步引擎挂起进
/// 队列（issue #856），依赖方 op 补齐后重投递自然重试（issue #861 / #980）。
/// 删除重放支持全部 kind（按行现状执行与本地删除同一协议，含 sell 回补 /
/// buy 与 convert 级联与持仓清理，issue #940 / #979）。
pub(crate) fn replay_command(conn: &Connection, command: &TransactionCommand) -> Result<()> {
    match command {
        TransactionCommand::Create {
            id,
            row,
            investment,
            convert,
        } => {
            create_protocol(
                conn,
                CreateForm::Replay {
                    id,
                    row,
                    investment: investment.as_ref(),
                    convert: convert.as_ref(),
                },
            )?;
            Ok(())
        }
        TransactionCommand::Update {
            id,
            row,
            investment,
            convert,
        } => {
            update_protocol(
                conn,
                id,
                UpdateForm::Replay {
                    row,
                    investment: investment.as_ref(),
                    convert: convert.as_ref(),
                },
            )?;
            Ok(())
        }
        TransactionCommand::Delete { id } => delete_within_transaction(conn, id, WriteForm::Replay),
    }
}

/// 计划装配 Replay 臂（协议分歧点①的 Replay 形态）：从命令携带的归一化行与语义
/// 字段装配 [`Plan`]，不重折算（源端折算，ADR-0091 决策 3）。
///
/// - 普通 kind：行原样为计划（落库即行本身，含余额缓存重算）；
/// - buy/sell：投资域重放形态计划重建（[`investment::replay_plan`]）——依赖在位
///   校验（数量为正、标的存在、账户为投资账户、可卖数量）以与本地写入同码的
///   码化错误上抛，由引擎挂起承接；买入每份成本随命令携带、不重算（重算需读
///   标的类型，属本地状态）；
/// - convert：重放形态转换计划重建（[`investment::replay_convert_plan`]）——
///   逐批次消耗按本地 FIFO 快照重建，源端算定的结转成本作权威比对基准
///   （不一致显式失败挂起，不静默错账，ADR-0099 决策 6）；旧版本设备产出的
///   转换 op 不携转换字段，缺失即显式失败挂起（kind 防御臂，与 schema 版本硬
///   检查双保险），不落半套副作用；
/// - dividend / split：协议守卫单点已先行拒绝（split 的 Replay 拒绝见创建协议），
///   此臂为穷尽兜底，同码错误表达不可达态（不引入 panic 构造，ADR-0060）。
fn replay_assembly(
    conn: &Connection,
    row: &NormalizedTransaction,
    investment: Option<&InvestmentCommandFields>,
    convert: Option<&ConvertCommandFields>,
) -> Result<Plan> {
    let norm_row = writer::NormalizedRow::try_from(row)?;
    match norm_row.kind {
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => Ok(Plan::Common(norm_row)),
        TransactionKind::Buy | TransactionKind::Sell => {
            let fields = investment_fields(investment)?;
            Ok(Plan::Investment(investment::replay_plan(
                conn,
                norm_row.kind,
                row,
                fields,
            )?))
        }
        TransactionKind::Convert => {
            let fields = convert_fields(convert)?;
            Ok(Plan::Investment(investment::replay_convert_plan(
                conn, row, fields,
            )?))
        }
        TransactionKind::Dividend | TransactionKind::Split => Err(kind_unsupported(norm_row.kind)),
    }
}

/// 投资命令字段解包（防御臂）：buy/sell 命令必携投资字段；缺失属载荷伪造或
/// 程序缺陷（产出侧永不产 None 的投资命令），fail loud 由引擎挂起承接。
fn investment_fields(
    investment: Option<&InvestmentCommandFields>,
) -> Result<&InvestmentCommandFields> {
    investment.ok_or_else(|| AppError::Invalid("投资命令缺少投资字段（程序缺陷）".into()))
}

/// 转换命令字段解包（防御臂）：convert 命令必携转换字段；缺失属旧版本设备载荷
/// 或程序缺陷（本地 plan 自 #980 起恒产出），码化失败由引擎挂起承接（ADR-0099
/// 决策 6 的 kind 防御臂——旧端 op 与 schema 版本硬检查双保险）。
fn convert_fields(convert: Option<&ConvertCommandFields>) -> Result<&ConvertCommandFields> {
    convert.ok_or_else(|| {
        AppError::coded(
            "transaction.convert-fields-missing",
            "该转换操作缺少同步所需的转换字段（产生自较早版本），无法在本机重放；请在来源设备上删除并重新录入该转换后再次同步",
        )
    })
}

/// dividend 未实现 / split 重放未收编（协议 kind 守卫单点的错误构造器）：创建/修改两协议
/// 与 Replay 装配兜底臂共用，两形态同码同文案由构造保证。
fn kind_unsupported(kind: TransactionKind) -> AppError {
    AppError::codedp(
        "transaction.kind-unsupported",
        format!("交易类型 {kind} 暂不支持（MVP 未实现）"),
        &[&kind.to_string()],
    )
}
