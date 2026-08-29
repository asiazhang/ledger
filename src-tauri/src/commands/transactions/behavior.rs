//! 交易类型行为层（issue #72 / spec #69 候选 2）：按 kind 收敛分派。
//!
//! 对外暴露**创建编排入口** [`create`]（issue #228 / ADR-0030：连接 + 输入进，
//! 顺序契约与事务边界内化为实现细节），以及三个模块内协作件：
//! - [`plan`]：校验 + 归一化（normalize→plan，交易行不落库；商户名归一化含参考数据字典的
//!   未命中即建，见下）；
//! - [`apply`]：应用计划的副作用（普通 kind 无副作用；buy 建仓 / sell 卖出匹配）；
//! - [`revert`]：回退一笔已存在交易（按旧 kind）的副作用（普通 kind 无副作用）。
//!
//! **嵌套感知事务（「保证处于事务中」，ADR-0030 决策 #2）**：[`create`] 检测连接
//! 事务状态——autocommit 则自持 BEGIN/COMMIT/ROLLBACK，`insert_row` 后 apply 中途
//! 失败整体回滚、无已落库交易行与半套副作用的中间态泄漏；已在事务中（批量导入的
//! 外层批次事务）则加入外层、失败直接返回错误，回滚归外层持有者（batch 是嵌套模式
//! 的唯一合法使用者）。提交点副作用（备份置脏，ADR-0032）归事务持有者：自持事务由
//! 连接层统一写入口 `db.write` 在提交点单点触发，嵌套模式由 batch 在自己 COMMIT 后
//! 触发——本模块对备份域零感知。
//!
//! 分派是薄而穷尽的 `match`（不引入 trait 注册表，避免过度设计）：
//! 普通 kind（income/expense/transfer/refund）经 Writer 接缝归一化；buy/sell 委托投资域
//! （`commands::investment` 的 prepare/apply/revert，正向分派保留）；`dividend` / `split`
//! 已声明但未实现，在此显式「暂不支持」拒绝——这是本重构唯一对外的可观测行为变化
//! （此前经交易接口创建 dividend/split 落入 [`writer::normalize`] 的通用兜底，返回语义不明的
//! 「仅处理通用交易类型」；现改为明确的「暂不支持」，两者都不落库）。
//!
//! 依赖方向：命令层（transactions → investment → 无反向）。行为协作件只接受连接、
//! 不内嵌事务——事务边界由编排入口（或修改/批量编排，迁移期）持有，行为层保证在这些
//! 事务内，买卖的行写入与 lot/匹配副作用同处一个事务。

use rusqlite::Connection;

use crate::commands::investment;
use crate::error::{AppError, Result};
use crate::models::TransactionInput;
use crate::transaction::amount::TransactionKind;
use crate::transaction::writer;

/// 计划：归一化后的交易行 + kind 特有副作用数据（不落库）。
pub(crate) enum Plan {
    /// 普通 kind（income/expense/transfer/refund）：无副作用。
    Common(writer::NormalizedRow),
    /// 投资 kind（buy/sell）：归一化行与副作用数据留在投资域计划中。
    Investment(investment::Plan),
}

impl Plan {
    /// 归一化交易行（供 [`writer::insert_row`] / [`writer::update_row`] 落库）。
    pub(crate) fn normalized_row(&self) -> Result<writer::NormalizedRow> {
        match self {
            Plan::Common(row) => Ok(row.clone()),
            Plan::Investment(p) => Ok(writer::NormalizedRow::try_from(p.normalized())?),
        }
    }
}

/// 创建一笔交易（IPC `create_transaction` / 批量导入批次循环，issue #228 / ADR-0030）。
///
/// 行为层创建编排入口：`plan → insert_row → apply` 的顺序契约在此单点可达，
/// 调用方只传连接与输入、处理报错。
///
/// 事务规则是「保证处于事务中」（嵌套感知，见模块注释）：autocommit 自持事务
/// （中途失败整体回滚）；外层事务中加入外层（失败直接返回错误，回滚归外层持有者）。
///
/// 可见性说明：函数本体 `pub`（供 [`super`] 以 `create_transaction_internal` 名字
/// 公开再导出，模块本身私有故不额外扩大可见面）。
pub fn create(conn: &Connection, input: TransactionInput) -> Result<String> {
    // is_autocommit()=true ⇔ 连接不在事务中（rusqlite 语义），据此选分支。
    let owns_tx = conn.is_autocommit();
    if owns_tx {
        conn.execute("BEGIN", [])?;
    }
    let result = create_within_transaction(conn, &input);
    match (result, owns_tx) {
        (Ok(id), false) => Ok(id),
        (Ok(id), true) => match conn.execute("COMMIT", []) {
            Ok(_) => Ok(id),
            // COMMIT 失败：尽力回滚清理残留（与批量编排同款），再上抛提交错误。
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                Err(e.into())
            }
        },
        // 自持事务中途失败：整体回滚，不留已落库交易行与半套副作用。
        (Err(e), true) => {
            conn.execute("ROLLBACK", [])?;
            Err(e)
        }
        // 嵌套模式：加入外层，失败直接返回错误——回滚归外层持有者。
        (Err(e), false) => Err(e),
    }
}

/// 创建协议本体：`plan → insert_row → apply`（无事务语义，由 [`create`] 嵌套感知包裹）。
fn create_within_transaction(conn: &Connection, input: &TransactionInput) -> Result<String> {
    let plan = plan(conn, input, None)?;
    let row = plan.normalized_row()?;
    let id = writer::insert_row(conn, &row)?;
    apply(conn, &id, &plan)?;
    // 搜索无索引（issue #196 全量扫描实现）：写入路径零额外工作，交易立即可搜。
    Ok(id)
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
/// 自然走到；商户创建与交易落库同处调用方持有的事务，中途回滚不残留碎商户。幂等重放不产生碎商户：批量导入命中去重的行不会
/// 走到本函数，同批内首行即建、后续行按名精确匹配复用。
///
/// 单点分派全部 8 种 kind：通用 kind 经 Writer 接缝 [`writer::normalize`]（金额>0、
/// transfer 目标账户、refund 继承原支出等校验 + 本位币折算）；buy/sell 委托投资域
/// [`investment::prepare`]（投资账户/数量/单价/可卖数量校验 + 折算）；
/// `dividend` / `split` 已声明但未实现，显式「暂不支持」报错——取代此前
/// [`writer::normalize`] 兜底的「仅处理通用交易类型」文案（唯一对外可观测变化）。
pub(crate) fn plan(
    conn: &Connection,
    input: &TransactionInput,
    existing_merchant_id: Option<&str>,
) -> Result<Plan> {
    let kind = input.kind;
    // 商户携带收口（issue #188 / ADR-0028 + #194 商户名）：expense / refund / income 可携带
    // 商户（merchant_id 或 merchant_name）；transfer / buy / sell / dividend / split 行为层
    // 拒绝（schema 层 merchant_id 允许 NULL、不设 kind 限制，放开无需再改表）。refund 携带
    // 的商户在 [`writer::normalize`] 里被原支出商户覆盖（继承语义），此处不拦截。
    if (input.merchant_id.is_some() || input.merchant_name.is_some())
        && !matches!(
            kind,
            TransactionKind::Income | TransactionKind::Expense | TransactionKind::Refund
        )
    {
        return Err(AppError::Invalid(format!("交易类型 {kind} 不能携带商户")));
    }
    match kind {
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => {
            // 商户名解析须在 kind 收口之后：非 income/expense/refund 的行在此前已拒绝，
            // 不会先建商户再拒绝（避免产生字典碎片）。名字与 id 同时提供属请求错误。
            // refund 继承原支出商户（writer::normalize 覆盖）：携带的商户（id 或名字）
            // 一律忽略，不解析、不即建——否则即建商户必成孤儿（issue #194）。
            let (merchant_id, pending_name) = if kind == TransactionKind::Refund {
                (None, None)
            } else {
                match (&input.merchant_name, &input.merchant_id) {
                    (Some(_), Some(_)) => {
                        return Err(AppError::Invalid(
                            "merchant_id 与 merchant_name 不可同时提供".into(),
                        ));
                    }
                    (Some(name), None) => {
                        match crate::commands::merchants::find_merchant_by_name(conn, name)? {
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
                    category_id: input.category_id.clone(),
                    merchant_id,
                    existing_merchant_id: existing_merchant_id.map(str::to_string),
                    refund_of_transaction_id: input.refund_of_transaction_id.clone(),
                    note: input.note.clone(),
                    date: input.date.clone(),
                },
            )?;
            // 行内校验全部通过后才即建商户：未命中名字在此落定（失败行不产生碎商户）。
            if let Some(name) = pending_name {
                norm.merchant_id = Some(crate::commands::merchants::create_merchant_by_name(
                    conn, &name,
                )?);
            }
            Ok(Plan::Common(norm))
        }
        TransactionKind::Buy | TransactionKind::Sell => {
            Ok(Plan::Investment(investment::prepare(conn, kind, input)?))
        }
        TransactionKind::Dividend | TransactionKind::Split => Err(AppError::Invalid(format!(
            "交易类型 {kind} 暂不支持（MVP 未实现）"
        ))),
    }
}

/// 应用计划的副作用（创建/修改落库后调用）。
pub(crate) fn apply(conn: &Connection, id: &str, plan: &Plan) -> Result<()> {
    match plan {
        Plan::Common(_) => Ok(()),
        Plan::Investment(p) => investment::apply(conn, id, p),
    }
}

/// 回退一笔已存在交易（按旧 kind）的副作用，供删除/修改前清理。
///
/// 普通 kind 与未实现的 dividend/split 无持仓副作用，为 no-op；
/// buy 守卫（已有部分卖出拒绝）+ 清理，sell 回补持仓扣减。
/// `partial_sold_msg` 为 buy 守卫的措辞（删除/修改场景文案不同）。
pub(crate) fn revert(
    conn: &Connection,
    id: &str,
    kind: TransactionKind,
    partial_sold_msg: &str,
) -> Result<()> {
    match kind {
        TransactionKind::Buy | TransactionKind::Sell => {
            investment::revert(conn, id, kind, partial_sold_msg)
        }
        // 普通 kind 与未实现的 dividend/split 无持仓副作用，为 no-op。
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund
        | TransactionKind::Dividend
        | TransactionKind::Split => Ok(()),
    }
}
