//! 交易类型行为层（issue #72 / spec #69 候选 2）：按 kind 收敛分派。
//!
//! 对外暴露**三个编排入口** [`create`] / [`update`] / [`delete`]（issue #228 / #229 /
//! ADR-0033：连接 + 输入进，返回终态或错误）——`revert → plan → 落库 → apply` 的顺序
//! 契约、事务边界、守卫文案全部内化为实现细节，调用方只传连接与输入、处理报错。
//! 模块内协作件 [`plan`]（校验 + 归一化）与 [`apply`]（应用副作用）为私有实现细节。
//!
//! **嵌套感知事务（「保证处于事务中」，ADR-0033 决策 #2）**：每个入口经
//! [`ensure_transaction`] 检测连接事务状态——autocommit 则自持 BEGIN/COMMIT/ROLLBACK
//! （创建的 insert_row 后 apply 中途失败、删除的持仓清理后软删 UPDATE 失败均整体回滚，
//! 无中间态泄漏）；已在事务中则加入外层、失败直接返回错误，回滚归外层持有者
//! （批量导入的批次事务与余额调整的外层事务壳是嵌套模式的合法使用者，issue #310）。
//! 提交点副作用（备份置脏，ADR-0032，issue #245 起含嵌套模式）统一归连接层写入口：
//! 调用方的 `db.write` 闭包返回 Ok 且已提交时单点触发——自持事务与批量嵌套同一形态，
//! 本模块对备份域零感知。
//!
//! **守卫文案按入口内化（ADR-0033 决策 #4）**：buy 已有部分卖出的拒绝文案是修改
//! 入口的实现细节（`PARTIAL_SOLD_CANNOT_UPDATE`），单点定义、不随调用方漂移；回退
//! 分派直接委托 [`investment::revert`]（其 match 已覆盖全部 kind，普通 kind 为
//! no-op），行为层不再另设 revert 转发层。删除入口不设部分卖出守卫（issue #940 /
//! ADR-0097）：sell 删除回补持仓、buy 删除级联软删其在用 sell——「已有部分卖出的
//! 买入禁删」随之退场。
//!
//! 分派是薄而穷尽的 `match`（不引入 trait 注册表，避免过度设计）：
//! 普通 kind（income/expense/transfer/refund）经 Writer 接缝归一化；buy/sell 委托投资域
//! （`investment` 域入口的 prepare/apply/revert，正向分派保留）；`dividend` / `split`
//! 已声明但未实现，在此显式「暂不支持」拒绝——这是 #72 重构唯一对外的可观测行为变化
//! （此前经交易接口创建 dividend/split 落入 [`writer::normalize`] 的通用兜底，返回语义不明的
//! 「仅处理通用交易类型」；现改为明确的「暂不支持」，两者都不落库）。
//!
//! 依赖方向：命令层（transactions → investment → 无反向）。行为层保证行写入与 lot/匹配
//! 副作用同处一个事务（入口自持，或加入调用方外层事务）。
//!
//! **结果证据外传（ADR-0044 决策 4，issue #331）**：[`plan`] 在入参带 `merchant_name`
//! 且未命中时即建商户（写 `merchants` 表，第四张参考表），「是否即建」作为
//! [`WriteEvidence::MerchantCreated`] 随创建/修改入口的自然返回值外传——壳层据此经
//! `signals_for` 判定发 `ledger:changed`（仅命中复用为零信号）；批量导入在批次层聚合
//! 「任一行即建」。可变容器与行为层直接发信号（拿不到 `AppHandle`）均已否决。

use rusqlite::Connection;
use rusqlite::OptionalExtension;

use super::command::{InvestmentCommandFields, TransactionCommand, record_local};
use super::model::{NormalizedTransaction, TransactionInput};
use crate::accounts::balance::{affected_accounts, refresh_account_balances};
use crate::db::now_iso;
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

/// buy 已有部分卖出的守卫文案——修改入口措辞（ADR-0033 决策 #4：按入口内化、
/// 行为层单点定义，调用方协议面不出现文案，同一入口同一文案不漂移）。
/// 删除入口曾有的 `PARTIAL_SOLD_CANNOT_DELETE`（`trade.partially-sold-delete`）
/// 随级联删除退场（issue #940 / ADR-0097）——删除不再拒绝，改为级联。
const PARTIAL_SOLD_CANNOT_UPDATE: &str = "该买入交易已有部分卖出，无法修改";
/// 上迹守卫的稳定错误码（issue #342 二期）：与文案同源单点，经 revert 下传。
const PARTIAL_SOLD_CANNOT_UPDATE_CODE: &str = "trade.partially-sold-update";

/// 计划：归一化后的交易行 + kind 特有副作用数据（不落库）。
enum Plan {
    /// 普通 kind（income/expense/transfer/refund）：无副作用。
    Common(writer::NormalizedRow),
    /// 投资 kind（buy/sell）：归一化行与副作用数据留在投资域计划中。
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

/// 「保证处于事务中」（嵌套感知，ADR-0033 决策 #2）：连接 autocommit 则自持
/// BEGIN/COMMIT/ROLLBACK（`f` 中途失败整体回滚）；已在事务中则加入外层、失败直接
/// 返回错误——回滚归外层持有者（批量导入的批次事务与余额调整的外层事务壳，
/// issue #310，是嵌套模式的合法使用者）。
///
/// `pub(crate)`（issue #855）：`sync_engine::apply_ops` 重放外来 op 时复用同一
/// 事务原语（命令执行 + op 落日志同事务原子），不另造第二份嵌套感知实现。
pub(crate) fn ensure_transaction<T>(conn: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
    // is_autocommit()=true ⇔ 连接不在事务中（rusqlite 语义），据此选分支。
    if !conn.is_autocommit() {
        return f();
    }
    conn.execute("BEGIN", [])?;
    match f() {
        Ok(v) => match conn.execute("COMMIT", []) {
            Ok(_) => Ok(v),
            // COMMIT 失败：尽力回滚清理残留（与批量编排同款），再上抛提交错误。
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                Err(e.into())
            }
        },
        // 自持事务中途失败：整体回滚，不留已落库交易行与半套副作用；
        // ROLLBACK 自身失败不遮蔽业务错误（与 COMMIT 失败分支同款，尽力回滚后上抛原错误）。
        Err(e) => {
            let _ = conn.execute("ROLLBACK", []);
            Err(e)
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

/// 创建一笔交易（IPC `create_transaction` / 批量导入批次循环 / 余额调整的交易写入，
/// issue #228 / #310 / ADR-0033）。
///
/// 行为层创建编排入口：`plan → insert_row → apply` 的顺序契约在此单点可达，
/// 调用方只传连接与输入、处理报错。事务规则见 [`ensure_transaction`]。
/// 返回值携带终态 id 与「是否即建商户」证据（ADR-0044 决策 4，issue #331）。
///
/// 可见性说明：三个入口函数本体 `pub`（供 [`super`] 以 `*_transaction_internal`
/// 名字公开再导出，模块本身私有故不额外扩大可见面）。
pub fn create(conn: &Connection, input: TransactionInput) -> Result<TransactionWrite> {
    ensure_transaction(conn, || create_within_transaction(conn, &input))
}

/// 创建协议本体：`plan → insert_row → apply → 产出 op`（无事务语义，由
/// [`ensure_transaction`] 包裹）。op 产出为最后一步：写与副作用全部成功后才追加，
/// 随同一事务提交/回滚（失败不残留 op）。
fn create_within_transaction(
    conn: &Connection,
    input: &TransactionInput,
) -> Result<TransactionWrite> {
    let (plan, merchant_created) = plan(conn, input, None)?;
    let row = plan.normalized_row()?;
    let id = writer::insert_row(conn, &row)?;
    apply(conn, &id, &plan)?;
    // 搜索（V018 两段式，issue #492）：Writer 接缝同写维护 note_pinyin 派生列，
    // 交易立即可搜（存量积压由读路径惰性回填兜底）。
    // op 产出接缝（issue #855 / ADR-0091）：本地写成功 → 动作连同归一化行
    // （含源端折算）追加进本机 OpLog；全部写路径经本入口收敛，产出不散落。
    record_local(conn, create_command(&id, &plan))?;
    Ok(TransactionWrite {
        id,
        evidence: WriteEvidence::MerchantCreated(merchant_created),
    })
}

/// 按 `id` 全字段替换一笔交易（IPC `update_transaction` / HTTP
/// `PUT /api/v1/transactions/{id}`，issue #229 / ADR-0033）。
///
/// 行为层修改编排入口：`revert → plan → update_row → apply` 的顺序契约与守卫文案
/// （`PARTIAL_SOLD_CANNOT_UPDATE`）在此单点可达，调用方只传连接、id 与输入、处理报错。
/// 事务规则见 [`ensure_transaction`]；旧 kind/商户的读取在事务内完成（消除读取与
/// BEGIN 之间的窗口，ADR-0033 决策 #5）。
///
/// 幂等键（`idempotency_key`）与内容哈希（`dedup_hash`）不作为
/// 可编辑字段——修改不重算去重身份，故修改后重跑同批导入（带幂等键）仍按同键去重、不产生重复。
/// 不存在或已软删除的 id 返回码化 NotFound（`transaction.not-found`）。
/// 返回「是否即建商户」证据（[`WriteEvidence::MerchantCreated`]，issue #331）。
pub fn update(conn: &Connection, id: &str, input: TransactionInput) -> Result<WriteEvidence> {
    ensure_transaction(conn, || update_within_transaction(conn, id, &input))
}

/// 修改协议本体：`revert → plan → update_row → apply`
/// （无事务语义，由 [`ensure_transaction`] 包裹）。
fn update_within_transaction(
    conn: &Connection,
    id: &str,
    input: &TransactionInput,
) -> Result<WriteEvidence> {
    // 读取旧交易 kind 与当前商户、当前保单（商户/保单用于「保持历史引用」判定：提交值
    // 与原值相同则跳过在用校验，已软删商户/保单的历史交易仍可修改其他字段），
    // 不存在或已删除返回 NotFound。读取在事务内（入口已保证处于事务中）。
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
            AppError::codedp_not_found("transaction.not-found", format!("交易不存在: {id}"), &[id])
        })?;

    // 先按旧 kind 回退持仓/卖出关联副作用，再按新 kind 校验并应用（跨 kind 修改避免孤儿持仓）；
    // buy 守卫（已有部分卖出拒绝）措辞与错误码为修改入口单点定义的文案。
    investment::revert(
        conn,
        id,
        old_kind,
        PARTIAL_SOLD_CANNOT_UPDATE_CODE,
        PARTIAL_SOLD_CANNOT_UPDATE,
    )?;
    let (plan, merchant_created) = plan_with_existing_refs(
        conn,
        input,
        old_merchant_id.as_deref(),
        old_policy_id.as_deref(),
    )?;
    let row = plan.normalized_row()?;
    writer::update_row(conn, id, &row)?;
    apply(conn, id, &plan)?;
    // op 产出接缝（issue #855 / ADR-0091）：修改成功 → 新行（含源端折算）随
    // update op 追加；随同一事务提交/回滚。
    record_local(conn, update_command(id, &plan))?;
    Ok(WriteEvidence::MerchantCreated(merchant_created))
}

/// 删除交易（软删除 `is_deleted=1`；IPC `delete_transaction` / HTTP
/// `DELETE /api/v1/transactions/{id}`，issue #229 / ADR-0033）。
///
/// 行为层删除编排入口：持仓副作用回退与软删 UPDATE 同处一个事务——回退成功后
/// 软删 UPDATE 中途失败整体回滚，不再出现「持仓已删而交易仍在」的中间态
/// （删除路径事务缺口修复，ADR-0033 决策 #3）。
///
/// 持仓副作用回退按 kind 分派（issue #940 / ADR-0097，部分修订 ADR-0013 删除条）：
/// - sell：回补其扣减的持仓并清空卖出关联——删除即撤销其全部持仓影响，
///   不再遗留幽灵占用（旧版「sell 删除不回补」是把买入永久锁死的根源）；
/// - buy：**级联**——其持仓批次的在用 sell 逐笔回退持仓副作用并随之软删
///   （各自 delete op 与余额刷新），「已有部分卖出的买入禁删」守卫退场；
/// - 其余 kind 无持仓副作用，直接软删。
///
/// 不存在的 id 返回码化 NotFound（HTTP 侧映射 404）。事务规则见
/// [`ensure_transaction`]。IPC 与 HTTP 端点共用本函数。
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || delete_within_transaction(conn, id))
}

/// 删除协议本体：`release_for_delete（sell 回补 / buy 级联）→ 级联软删在用 sell →
/// 主行软删`（无事务语义，由 [`ensure_transaction`] 包裹）。
fn delete_within_transaction(conn: &Connection, id: &str) -> Result<()> {
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

    // 持仓副作用回退（issue #940 / ADR-0097）：sell 回补持仓扣减；buy 消费其
    // 持仓批次的在用 sell（逐笔回退）并整批清理批次与匹配，返回级联对象 id 列表。
    let cascaded_sell_ids = match kind {
        TransactionKind::Buy | TransactionKind::Sell => {
            investment::release_for_delete(conn, id, kind)?
        }
        _ => Vec::new(),
    };
    // 级联软删：被消化的在用 sell 逐笔软删（各自余额刷新与 delete op），先于主行
    // 落 op——重放端按 op 顺序先删 sell 再删 buy，级联与显式 op 不打架。
    for sell_id in &cascaded_sell_ids {
        soft_delete_transaction_row(conn, sell_id)?;
    }
    // 搜索（V018 两段式，issue #492）：候选流带 is_deleted 口径过滤，软删即刻
    // 生效，删除的交易不再可搜（拼音列非本路径维护字段，无需处理）。
    soft_delete_transaction_row(conn, id)
}

/// 软删单笔交易行 + 余额刷新 + delete op（删除编排的行级步骤，主删与级联删共用）。
///
/// 行读取（`is_deleted=0` 存在性 + 账户引用对）→ 软删 UPDATE → 受影响账户余额重算
/// （issue #491 / ADR-0067）→ delete op 追加（issue #855 / ADR-0091）。级联删除的
/// 被级联行亦各自留痕：同步端按同一 delete 协议逐笔收敛，无需感知级联语义。
fn soft_delete_transaction_row(conn: &Connection, id: &str) -> Result<()> {
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
    // op 产出接缝（issue #855 / ADR-0091）：删除成功 → delete op（实体 id）
    // 追加；随同一事务提交/回滚。
    record_local(conn, TransactionCommand::Delete { id: id.to_string() })
}

/// 校验并归一化一笔交易输入为计划（交易行不落库）。
///
/// `existing_merchant_id`：修改路径该行当前的商户 id（创建路径传 None）——提交商户
/// 与其相同视为保持历史引用（软删商户的历史交易仍可修改其他字段，见
/// [`writer::normalize`] 的商户校验）；改选其他商户按新选择校验在用。
///
/// 商户名归一化（AI 导入契约，issue #194）：输入带 `merchant_name` 时在此解析为
/// `merchant_id`——精确匹配在用商户名，命中复用、未命中即建。两段式避免碎商户：
/// 先查（[`merchants::find_merchant_by_name`]），未命中则等行内校验全部通过后
/// 再即建（[`merchants::create_merchant_by_name`]）——金额非法等校验失败的行
/// 不会残留无引用的商户行。这是「无副作用」的唯一例外：商户字典即建是参考数据
/// 归一化，收口在行为层使全部写路径（HTTP 批量导入 / IPC 单笔创建 / 按 id 修改）
/// 自然走到；商户创建与交易落库同处入口持有的事务，中途回滚不残留碎商户。幂等重放不产生碎商户：批量导入命中去重的行不会
/// 走到本函数，同批内首行即建、后续行按名精确匹配复用。
///
/// 单点分派全部 8 种 kind：通用 kind 经 Writer 接缝 [`writer::normalize`]（金额>0、
/// transfer 目标账户、refund 继承原支出等校验 + 本位币折算）；buy/sell 委托投资域
/// [`investment::prepare`]（投资账户/数量/单价/可卖数量校验 + 折算）；
/// `dividend` / `split` 已声明但未实现，显式「暂不支持」报错——取代此前
/// [`writer::normalize`] 兜底的「仅处理通用交易类型」文案（唯一对外可观测变化）。
/// 参考数据携带准入（商户/保单/分类，issue #188/#361/#582）在本函数内按 kind 单点
/// 判定：准入集外的 kind 携带对应引用即码化拒绝，创建/修改/批量导入各写入口经本函数
/// 自然共用同一收口，不另设第二份判定。
/// 返回 `(计划, 是否即建商户)`：后者即 [`WriteEvidence::MerchantCreated`] 的载荷——
/// 仅「入参带 `merchant_name` 且未命中、行内校验通过后落定即建」为真；命中复用、
/// 直接带 `merchant_id`、refund 继承忽略、不涉商户的 kind 一律为假。
fn plan(
    conn: &Connection,
    input: &TransactionInput,
    existing_merchant_id: Option<&str>,
) -> Result<(Plan, bool)> {
    plan_with_existing_refs(conn, input, existing_merchant_id, None)
}

/// [`plan`] 的全量形态：修改路径额外传该行当前的保单 id（保单「保持历史引用」判定，
/// 与商户同款语义）。创建路径传 None。
fn plan_with_existing_refs(
    conn: &Connection,
    input: &TransactionInput,
    existing_merchant_id: Option<&str>,
    existing_policy_id: Option<&str>,
) -> Result<(Plan, bool)> {
    let kind = input.kind;
    // 商户携带收口（issue #188 / ADR-0028 + #194 商户名 + #875 / ADR-0092 transfer）：
    // expense / refund / income / transfer 可携带商户（merchant_id 或 merchant_name）；
    // buy / sell / dividend / split 行为层拒绝（schema 层 merchant_id 允许 NULL、不设
    // kind 限制，放开无需再改表）。transfer 放开是纯 kind 判定，**不引入账户类型条件**
    // ——普通转账与借贷转账（receivable/debt 账户派生视角）共用同一收口，借贷关联
    // 语义只是检索与展示指针（见 docs/adr/0092）。refund 携带的商户在
    // [`writer::normalize`] 里被原支出商户覆盖（继承语义），此处不拦截。
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
    // 分类携带收口（issue #582）：expense / income 可携带分类；transfer / buy / sell
    // 行为层拒绝（schema 层 category_id 允许 NULL、不设 kind 限制，放开无需再改表）——
    // 转账与资本变动没有「花在哪类」的语义，无分类交易因此不进分类聚合。refund 携带
    // 的分类在 [`writer::normalize`] 里被原支出分类覆盖（继承语义），此处不拦截。
    // dividend / split 携带分类时本拒绝先于「暂不支持」触发（比照商户收口先例）。
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
    // 保单携带准入（issue #361 / ADR-0051 决策 3）：仅 income / expense 可携带可选保单
    // 引用；transfer / buy / sell / dividend / split 行为层拒绝（保单口径不被资本变动
    // 污染）。refund 不在准入集：保单现金流入记 income 挂单而非 refund（ADR-0051 决策 4），
    // 且 refund 继承原支出字段，携带保单无意义，拒绝保持准入集最小。
    if input.policy_id.is_some()
        && !matches!(kind, TransactionKind::Income | TransactionKind::Expense)
    {
        return Err(AppError::codedp(
            "transaction.policy-unsupported",
            format!("交易类型 {kind} 不能挂保单"),
            &[&kind.to_string()],
        ));
    }
    match kind {
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => {
            // 商户名解析须在 kind 收口之后：非 income/expense/refund/transfer 的行在此前
            // 已拒绝，不会先建商户再拒绝（避免产生字典碎片）。名字与 id 同时提供属请求错误。
            // refund 继承原支出商户（writer::normalize 覆盖）：携带的商户（id 或名字）
            // 一律忽略，不解析、不即建——否则即建商户必成孤儿（issue #194）。
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
        TransactionKind::Buy | TransactionKind::Sell => {
            // 投资 kind 不涉商户（行为层 kind 收口已拒绝携带），证据恒假。
            Ok((
                Plan::Investment(investment::prepare(conn, kind, input)?),
                false,
            ))
        }
        TransactionKind::Dividend | TransactionKind::Split => Err(AppError::codedp(
            "transaction.kind-unsupported",
            format!("交易类型 {kind} 暂不支持（MVP 未实现）"),
            &[&kind.to_string()],
        )),
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
fn command_parts(plan: &Plan) -> (NormalizedTransaction, Option<InvestmentCommandFields>) {
    match plan {
        Plan::Common(r) => (NormalizedTransaction::from(r), None),
        Plan::Investment(p) => {
            let fields = match p {
                investment::Plan::Buy(b) => InvestmentCommandFields {
                    instrument_id: b.instrument_id.clone(),
                    quantity: b.quantity,
                    price_cents: b.price_cents,
                    fee_cents: b.fee_cents,
                    cost_per_unit_cents: Some(b.cost_per_unit_cents),
                },
                investment::Plan::Sell(s) => InvestmentCommandFields {
                    instrument_id: s.instrument_id.clone(),
                    quantity: s.quantity,
                    price_cents: s.price_cents,
                    fee_cents: s.fee_cents,
                    cost_per_unit_cents: None,
                },
            };
            (p.normalized().clone(), Some(fields))
        }
    }
}

/// 创建命令构造（行为层创建协议专用）。
fn create_command(id: &str, plan: &Plan) -> TransactionCommand {
    let (row, investment) = command_parts(plan);
    TransactionCommand::Create {
        id: id.to_string(),
        row,
        investment,
    }
}

/// 修改命令构造（行为层修改协议专用）。
fn update_command(id: &str, plan: &Plan) -> TransactionCommand {
    let (row, investment) = command_parts(plan);
    TransactionCommand::Update {
        id: id.to_string(),
        row,
        investment,
    }
}

/// 同步重放入口：执行外来交易命令（issue #855 / ADR-0091）。
///
/// 与本地写入共用同一编排协议（`revert → 落库 → apply` 与既有守卫文案），但
/// 三处刻意差异构成重放形态：
/// - **折算随命令携带**（源端折算）：行数据原样落库，不查本地汇率表——重放
///   不依赖本地汇率表状态与设备配置（ADR-0091 决策 3）；
/// - **实体 id 随命令携带**：创建经 [`writer::insert_row_with_id`] 落库，两端
///   对同一笔交易收敛到同一行；
/// - **不产出 op**：外来 op 由同步引擎（`sync_engine::apply_ops`）在重放事务
///   内落日志，命令执行不得再追加本地 op。
///
/// 投资 kind（buy/sell）的重放（issue #861）：持仓/卖出关联副作用经投资域
/// 重放形态计划重建（[`investment::replay_plan`]）装配后走同一 `apply`——
/// 依赖在位校验（标的、投资账户、可卖数量）以与本地写入同码的码化错误上抛，
/// 由同步引擎挂起进队列（issue #856），依赖方 op 补齐后重投递自然重试；
/// 买入每份成本随命令携带、不重算。删除重放支持全部 kind（按行现状执行
/// 与本地删除同一协议，含 sell 回补 / buy 级联与持仓清理，issue #940）。
pub(crate) fn replay_command(conn: &Connection, command: &TransactionCommand) -> Result<()> {
    match command {
        TransactionCommand::Create {
            id,
            row,
            investment,
        } => replay_create(conn, id, row, investment.as_ref()),
        TransactionCommand::Update {
            id,
            row,
            investment,
        } => replay_update(conn, id, row, investment.as_ref()),
        TransactionCommand::Delete { id } => delete_within_transaction(conn, id),
    }
}

/// 重放创建：普通 kind 原样落库（含余额缓存重算）；投资 kind 经重放形态计划
/// 重建装配副作用后落库；dividend / split 与本地写入同码拒绝（kind-unsupported
/// ——本地 plan 从不产出这两种命令，此处是伪造/漂移载荷的防御臂，防绕过
/// 「暂不支持」守卫直落交易行）。落地前校验账户引用存活（issue #856：
/// 「往已删账户记账」码化拒绝，引擎挂起待裁决，不自动复活已删账户）。
fn replay_create(
    conn: &Connection,
    id: &str,
    row: &NormalizedTransaction,
    investment: Option<&InvestmentCommandFields>,
) -> Result<()> {
    let norm_row = writer::NormalizedRow::try_from(row)?;
    // 伪造/漂移载荷的防御臂先于依赖校验：dividend / split 与本地写入同码拒绝
    // （kind-unsupported——本地 plan 从不产出这两种命令），不误报账户缺失。
    if matches!(
        norm_row.kind,
        TransactionKind::Dividend | TransactionKind::Split
    ) {
        return Err(kind_unsupported(norm_row.kind));
    }
    writer::validate_accounts_alive(
        conn,
        &norm_row.account_id,
        norm_row.to_account_id.as_deref(),
        norm_row.funding_account_id.as_deref(),
    )?;
    match norm_row.kind {
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => writer::insert_row_with_id(conn, id, &norm_row),
        TransactionKind::Buy | TransactionKind::Sell => {
            let fields = investment_fields(investment)?;
            let plan = investment::replay_plan(conn, norm_row.kind, row, fields)?;
            writer::insert_row_with_id(conn, id, &norm_row)?;
            investment::apply(conn, id, &plan)
        }
        TransactionKind::Dividend | TransactionKind::Split => Err(kind_unsupported(norm_row.kind)),
    }
}

/// 重放修改：存在性守卫与本地修改同款（不存在或已软删返回码化 NotFound）；
/// 先按旧 kind 回退副作用（含 buy 部分卖出守卫，同码同文案），再按新 kind
/// 装配落库——与本地修改协议同序（`revert → 校验 → 落库 → apply`）；
/// dividend / split 目标与本地修改同码拒绝（防御臂同 [`replay_create`]）。
fn replay_update(
    conn: &Connection,
    id: &str,
    row: &NormalizedTransaction,
    investment: Option<&InvestmentCommandFields>,
) -> Result<()> {
    let (old_kind,): (TransactionKind,) = conn
        .query_row(
            "SELECT kind FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| Ok((r.get(0)?,)),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::codedp_not_found("transaction.not-found", format!("交易不存在: {id}"), &[id])
        })?;
    let new_kind = row.kind;
    if matches!(new_kind, TransactionKind::Dividend | TransactionKind::Split) {
        return Err(kind_unsupported(new_kind));
    }
    // 先按旧 kind 回退持仓/卖出关联副作用（普通 kind 为 no-op），再落新行。
    investment::revert(
        conn,
        id,
        old_kind,
        PARTIAL_SOLD_CANNOT_UPDATE_CODE,
        PARTIAL_SOLD_CANNOT_UPDATE,
    )?;
    let norm_row = writer::NormalizedRow::try_from(row)?;
    // 账户引用存活守卫（issue #856，与重放创建同款）：修改不得把交易改挂到
    // 已删账户上（含出资端，issue #935）。
    writer::validate_accounts_alive(
        conn,
        &norm_row.account_id,
        norm_row.to_account_id.as_deref(),
        norm_row.funding_account_id.as_deref(),
    )?;
    match new_kind {
        TransactionKind::Buy | TransactionKind::Sell => {
            let fields = investment_fields(investment)?;
            let plan = investment::replay_plan(conn, new_kind, row, fields)?;
            writer::update_row(conn, id, &norm_row)?;
            investment::apply(conn, id, &plan)
        }
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => writer::update_row(conn, id, &norm_row),
        // 入口已先行拒绝（dividend / split 同码防御臂）；穷尽分支不引入 panic
        // 构造（ADR-0060），以同码错误表达不可达态。
        TransactionKind::Dividend | TransactionKind::Split => Err(kind_unsupported(new_kind)),
    }
}

/// 投资命令字段解包（防御臂）：buy/sell 命令必携投资字段；缺失属载荷伪造或
/// 程序缺陷（产出侧永不产 None 的投资命令），fail loud 由引擎挂起承接。
fn investment_fields(
    investment: Option<&InvestmentCommandFields>,
) -> Result<&InvestmentCommandFields> {
    investment.ok_or_else(|| AppError::Invalid("投资命令缺少投资字段（程序缺陷）".into()))
}

/// dividend / split 未实现（与本地 plan 同码同文案，防御臂单点复用）。
fn kind_unsupported(kind: TransactionKind) -> AppError {
    AppError::codedp(
        "transaction.kind-unsupported",
        format!("交易类型 {kind} 暂不支持（MVP 未实现）"),
        &[&kind.to_string()],
    )
}
