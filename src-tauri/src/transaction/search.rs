//! 查询执行：两段式取行（V018，issue #492）——第一段最小列流式匹配 + 第二段当前页回表。
//!
//! 语义契约仍是 ADR-0027 统一模糊搜索规格（原文连续子串 ∨ 拼音首字母子序列，
//! 词条之间 AND、字段之间 OR，多音字前字规则），本模块只改**实现形态**（修订
//! ADR-0027「不预留索引层逃生舱」实现条款；子序列与倒排索引不兼容、FTS5 仍排除）：
//!
//! 1. **第一段（匹配段）**：SQL 只取单表最小列（id + note + note_pinyin + 三个
//!    引用列），沿 V018 搜索覆盖索引 `idx_transactions_note_search` index-only
//!    流式扫描（列表序供序、零回表；50 万笔实测 860ms → 233ms），逐行按统一语义
//!    契约过滤、命中计数 `total`、仅收集当前页 id。可搜索名字与软删口径（账户名/
//!    商户名/分类删除状态）改为搜索开始时一次性读取小参考表（个位数到千行量级）
//!    成 Rust 字典，替代逐行 JOIN——50 万候选流上 accounts/categories JOIN 即取行
//!    主要成本（实测单表扫描 25ms vs 带 JOIN 1170ms）；字典每次搜索新建，账户/
//!    商户改名即刻生效的语义不变。备注拼音子序列路径消费 V018 冗余列
//!    `note_pinyin`（Writer 接缝同写维护），免逐行重算拼音；列缺失（存量未回填/
//!    派生漂移）时现算兑底，语义不受回填进度影响。
//! 2. **第二段（展示段）**：仅为当前页（≤ page_size）命中 id 回表取展示列
//!    （`Transaction::from_row` 的 18 列，无 JOIN）。
//! 3. **惰性回填**：存量行的 `note_pinyin` 积压由搜索读路径按 V018 探针索引
//!    分批补齐（每批一个事务，内存有界）；回填属派生数据维护，失败仅降级为
//!    现算兑底并记日志，不影响搜索结果。同一回填核心另暴露为显式一键修复
//!    [`repair_note_pinyin`]（issue #513，设置页入口）：幂等回填全部积压并
//!    返回报告（回填行数 / 是否收敛 / 失败原因），作为回填失败时可触发的
//!    恢复手段。
//!
//! 热路径纪律：行内文本一律经 `get_ref` 借用匹配、词条与字典在搜索开始时一次性
//! 小写化/拼音化，命中且落当前页窗口才分配 id——50 万候选流上每行分配会吞掉
//! 索引收益。内存 O(当前页 + 小参考字典)（流式匹配、仅页 id 物化，ADR-0027
//! 修订记录口径不变；字典体量随账户/分类/商户字典而非交易流水增长）。

use std::collections::HashMap;

use rusqlite::Connection;
use rusqlite::types::Value;

use super::model::{
    NotePinyinRepairFailure, NotePinyinRepairReport, NotePinyinRepairStage, Transaction,
    TransactionSearchResult,
};
use crate::db::query::FromRow;
use crate::error::Result;

use super::search_text::{is_subsequence, pinyin_initials, split_terms};

pub use search_transactions_internal as search_transactions;

/// 每页条数上限（防呆，防止极端输入拖垮查询）。
const MAX_PAGE_SIZE: usize = 200;

/// 惰性回填单批行数：每批一个独立事务，内存有界且可与其它写路径交错。
const BACKFILL_BATCH: i64 = 2000;

/// 小写化词条（搜索开始时一次性准备，热路径免逐行分配）。
struct TermLowered {
    lower: String,
}

/// 可搜索名字字典条目：小写化名字 + 拼音首字母串（均搜索开始时算好，热路径
/// 免逐行分配）。名字即时读取语义由「字典每次搜索新建」保证（改名即刻生效）。
struct DictEntry {
    name_lower: String,
    pinyin: String,
}

/// 搜索字典：可搜索名字与软删口径的小参考表，搜索开始时一次性读取。
/// - 账户：id → (条目, 是否软删)——账户名即时读取（改名即刻生效），软删账户
///   的交易不可搜（与 `JOIN accounts a ON … AND a.is_deleted = 0` 等价）；
/// - 分类：id → 是否软删——软删分类名下的交易不可搜（与
///   `(c.is_deleted = 0 OR c.id IS NULL)` 等价）；
/// - 商户：id → 条目（含软删商户——历史交易仍可搜，与既有 LEFT JOIN 等价，
///   无 is_deleted 过滤）。
struct SearchDicts {
    accounts: HashMap<String, (DictEntry, bool)>,
    categories: HashMap<String, bool>,
    merchants: HashMap<String, DictEntry>,
}

fn dict_rows<T>(
    conn: &Connection,
    sql: &str,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let mut stmt = conn.prepare(sql)?;
    let mapped = stmt.query_map([], map)?;
    Ok(mapped.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn load_search_dicts(conn: &Connection) -> Result<SearchDicts> {
    let entry = |name: String| DictEntry {
        name_lower: name.to_lowercase(),
        pinyin: pinyin_initials(&name),
    };
    let mut accounts = HashMap::new();
    for (id, name, deleted) in dict_rows(conn, "SELECT id, name, is_deleted FROM accounts", |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })? {
        accounts.insert(id, (entry(name), deleted != 0));
    }
    let mut categories = HashMap::new();
    for (id, deleted) in dict_rows(conn, "SELECT id, is_deleted FROM categories", |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })? {
        categories.insert(id, deleted != 0);
    }
    let mut merchants = HashMap::new();
    for (id, name) in dict_rows(conn, "SELECT id, name FROM merchants", |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        merchants.insert(id, entry(name));
    }
    Ok(SearchDicts {
        accounts,
        categories,
        merchants,
    })
}

/// 第一段 SQL：单表最小列候选流（不物化展示列、无 JOIN），软删除交易口径与
/// 可选金额/日期过滤与交易列表一致，按交易日期降序（created_at、id 兜底，防
/// 同秒批量写入翻页漂移）预排序。
///
/// `pinned = true`（含关键字词条路径）时以 `INDEXED BY` 钉定 V018 搜索覆盖索引：
/// 子序列语义决定全量扫描本质，钉定使排序由索引序满足（无临时 B-tree）且扫描
/// index-only（零回表），并防 planner 在统计边际上摇摆（先例：V016 月度表达式
/// 索引钉定）；仅金额/日期筛选路径（pinned = false）保留 planner 自由——筛选
/// 选择性依赖数据分布，量筛选索引可能更优。账户/分类/商户侧口径由
/// [`SearchDicts`] 在 Rust 层逐行判定（JOIN 即 50 万候选流的取行主要成本，
/// 实测 25ms → 1170ms）。
pub(super) fn stage1_sql(where_clauses: &[&str], pinned: bool) -> String {
    let from = if pinned {
        "FROM transactions t INDEXED BY idx_transactions_note_search"
    } else {
        "FROM transactions t"
    };
    format!(
        "SELECT t.id,t.note,t.note_pinyin,t.account_id,t.merchant_id,t.category_id \
         {from} \
         WHERE {} \
         ORDER BY t.date DESC, t.created_at DESC, t.id DESC",
        where_clauses.join(" AND ")
    )
}

/// 已小写词条对目标的子序列判定（热路径）。语义与 [`is_subsequence`] 一致
/// （两侧小写化后逐字符有序匹配）：`pattern` 已小写；`target` 无大写字符时
/// 小写化为恒等、直接免分配匹配，含大写（派生列脏值等冷情形）降级为原分配
/// 路径。极小例外：titlecase 字符（如 'ǅ'）非 `is_uppercase` 而小写化非恒等，
/// 快路径按原字符参与匹配；拼音串恒小写不受影响，仅备注原文恰含此类冷僻字符
/// 时可感（热路径免逐字符小写化的取舍）。
fn is_subsequence_lower(pattern_lower: &str, target: &str) -> bool {
    if target.chars().any(char::is_uppercase) {
        return is_subsequence(pattern_lower, target);
    }
    let mut chars = target.chars();
    pattern_lower.chars().all(|p| chars.any(|t| t == p))
}

/// 原文对已小写词条的连续子串判定（热路径）。语义与
/// `text.to_lowercase().contains(&term.to_lowercase())` 一致：`text` 无大写
/// 字符时直接子串匹配，含大写（冷情形）降级为分配路径。极小例外同
/// [`is_subsequence_lower`]：titlecase 字符快路径按原字符匹配。
fn contains_lower(text: &str, term_lower: &str) -> bool {
    if text.chars().any(char::is_uppercase) {
        return text.to_lowercase().contains(term_lower);
    }
    text.contains(term_lower)
}

/// 词条对一笔第一段候选判定（统一语义契约，字段 OR；词条 AND 由调用方组合）：
/// - 备注字段：原文连续子串 ∨ 备注拼音首字母串的子序列（V018：拼音串优先取
///   冗余列，缺失时现算兜底——子序列判定语义与列来源无关）；
/// - 转出账户名 / 商户名：字典预计算的小写名与拼音串（与 [`term_matches_text`]
///   语义一致——原文子串 ∨ 现算拼音首字母子序列；名字体量小且字典每次搜索
///   新建，改名即刻生效）。
///
/// `account_name` / `merchant_name` 由调用方按 [`SearchDicts`] 解析并完成行级
/// 口径过滤（账户在用、分类未删），本函数只做词条匹配。
fn term_matches_borrowed(
    term: &TermLowered,
    note: Option<&str>,
    note_pinyin: Option<&str>,
    account: &DictEntry,
    merchant: Option<&DictEntry>,
) -> bool {
    let note_hit = note.is_some_and(|note| {
        contains_lower(note, &term.lower)
            || is_subsequence_lower(&term.lower, note_pinyin.unwrap_or(&pinyin_initials(note)))
    });
    note_hit
        || account.name_lower.contains(&term.lower)
        || is_subsequence_lower(&term.lower, &account.pinyin)
        || merchant.is_some_and(|m| {
            m.name_lower.contains(&term.lower) || is_subsequence_lower(&term.lower, &m.pinyin)
        })
}

/// 备注拼音积压探测（V018 partial 索引 `idx_transactions_note_pinyin_backlog`
/// 支撑，恒 O(1)）：true = 仍有「有备注且拼音列 NULL」的积压行；无备注行的
/// NULL 列不构成积压（拼音串仅由备注派生）。
fn probe_note_pinyin_backlog(conn: &Connection) -> Result<bool> {
    let hit: i64 = conn.query_row(
        "SELECT EXISTS(\
         SELECT 1 FROM transactions WHERE note_pinyin IS NULL AND note IS NOT NULL)",
        [],
        |r| r.get(0),
    )?;
    Ok(hit != 0)
}

/// 阶段化失败原因（底层错误消息原样透传，阶段供前端本地化）。
fn repair_failure(
    stage: NotePinyinRepairStage,
    e: impl std::fmt::Display,
) -> NotePinyinRepairFailure {
    NotePinyinRepairFailure {
        stage,
        message: e.to_string(),
    }
}

/// 收尾：最终收敛探测后组装报告（失败路径同样如实报告剩余积压；探测再失败
/// 时收敛位保守置 false，原始失败原因优先保留）。
fn finish_repair(
    conn: &Connection,
    backfilled: u64,
    failure: Option<NotePinyinRepairFailure>,
) -> NotePinyinRepairReport {
    let converged = match probe_note_pinyin_backlog(conn) {
        Ok(has_backlog) => !has_backlog,
        Err(e) => {
            tracing::warn!(error = %e, "备注拼音收敛探测失败（收敛位保守置否）");
            false
        }
    };
    NotePinyinRepairReport {
        backfilled,
        converged,
        failure,
    }
}

/// 备注拼音一键修复（issue #513，域接口）：显式回填全部积压并返回报告
/// （回填行数 / 是否收敛 / 失败原因）。幂等——仅补「拼音列仍为 NULL」的行，
/// 重复执行零回填、已回填行（含手工脏值）原样保留；分批事务与失败纪律同惰性
/// 回填：任一批失败记 warn、终止本轮回填、报告携带失败阶段与底层错误，不静默。
/// 派生数据维护不置脏（不触发备份）。搜索入口的惰性回填消费核心
/// [`backfill_note_pinyin`]（报告在搜索路径不消费，失败已 warn，语义由现算
/// 兜底保障），本入口只以「一键修复」领域语言显式触发同一实现并取报告。
pub fn repair_note_pinyin(conn: &Connection) -> NotePinyinRepairReport {
    backfill_note_pinyin(conn)
}

/// 备注拼音分批回填核心（惰性回填与一键修复的共享实现，见
/// [`repair_note_pinyin`] 文档）：返回回填行数 / 是否收敛 / 失败原因的报告。
fn backfill_note_pinyin(conn: &Connection) -> NotePinyinRepairReport {
    match probe_note_pinyin_backlog(conn) {
        // 已收敛：探测恒 O(1)，直接出报告（免二次探测）。
        Ok(false) => {
            return NotePinyinRepairReport {
                backfilled: 0,
                converged: true,
                failure: None,
            };
        }
        Ok(true) => {}
        Err(e) => {
            tracing::warn!(error = %e, "备注拼音回填探测失败");
            return finish_repair(
                conn,
                0,
                Some(repair_failure(NotePinyinRepairStage::Probe, e)),
            );
        }
    }
    tracing::info!("备注拼音列存在积压，开始回填（分批事务）");
    let mut backfilled: u64 = 0;
    loop {
        let rows: Vec<(String, String)> = match (|| {
            let mut stmt = conn.prepare(
                "SELECT id,note FROM transactions \
                 WHERE note_pinyin IS NULL AND note IS NOT NULL LIMIT ?1",
            )?;
            let mapped = stmt.query_map([BACKFILL_BATCH], |r| Ok((r.get(0)?, r.get(1)?)))?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()
        })() {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(error = %e, "备注拼音回填读取积压失败（终止本轮回填）");
                return finish_repair(
                    conn,
                    backfilled,
                    Some(repair_failure(NotePinyinRepairStage::Read, e)),
                );
            }
        };
        if rows.is_empty() {
            break;
        }
        let tx = match conn.unchecked_transaction() {
            Ok(tx) => tx,
            Err(e) => {
                tracing::warn!(error = %e, "备注拼音回填开事务失败（终止本轮回填）");
                return finish_repair(
                    conn,
                    backfilled,
                    Some(repair_failure(NotePinyinRepairStage::Begin, e)),
                );
            }
        };
        let mut changed = 0usize;
        let mut batch_err = None;
        for (id, note) in &rows {
            // 仅回填仍为 NULL 的行；updated_at 不动（纯派生列补齐，非业务写）。
            let res = tx.execute(
                "UPDATE transactions SET note_pinyin = ?1 \
                 WHERE id = ?2 AND note_pinyin IS NULL AND note = ?3",
                rusqlite::params![pinyin_initials(note), id, note],
            );
            match res {
                Ok(n) => changed += n,
                Err(e) => {
                    batch_err = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = batch_err {
            tracing::warn!(error = %e, "备注拼音回填写入失败（终止本轮回填）");
            // tx drop → 回滚本批，剩余积压由收敛探测如实报告。
            return finish_repair(
                conn,
                backfilled,
                Some(repair_failure(NotePinyinRepairStage::Write, e)),
            );
        }
        if let Err(e) = tx.commit() {
            tracing::warn!(error = %e, "备注拼音回填提交失败（终止本轮回填，本批回滚）");
            return finish_repair(
                conn,
                backfilled,
                Some(repair_failure(NotePinyinRepairStage::Commit, e)),
            );
        }
        backfilled += changed as u64;
        if changed == 0 {
            // 全批皆已被补齐（防御：防同批死循环）。
            break;
        }
        if rows.len() < BACKFILL_BATCH as usize {
            break;
        }
    }
    finish_repair(conn, backfilled, None)
}

/// 第二段：仅为当前页命中 id 回表取展示列（`Transaction::from_row` 的 18 列，
/// 无 JOIN）。输出保持第一段给定的日期降序（page_ids 顺序），页内缺行（理论上
/// 不可达：id 来自同一连接刚流式扫过的候选）安静跳过。
fn fetch_display_rows(conn: &Connection, page_ids: &[String]) -> Result<Vec<Transaction>> {
    if page_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = page_ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT t.id,t.kind,t.amount_cents,t.currency_code,t.amount_native_cents,t.account_id,\
         t.to_account_id,t.category_id,t.refund_of_transaction_id,t.note,t.date,t.created_at,\
         t.updated_at,t.version,t.device_id,t.is_deleted,t.merchant_id,t.policy_id \
         FROM transactions t WHERE t.id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<Value> = page_ids.iter().map(|id| id.clone().into()).collect();
    let rows = stmt.query_map(
        rusqlite::params_from_iter(params.iter()),
        Transaction::from_row,
    )?;
    let mut by_id: HashMap<String, Transaction> = HashMap::with_capacity(page_ids.len());
    for row in rows {
        let txn = row?;
        by_id.insert(txn.id.clone(), txn);
    }
    Ok(page_ids
        .iter()
        .filter_map(|id| by_id.get(id).cloned())
        .collect())
}

/// 服务端分页搜索交易。词条之间 AND，每词条对备注/转出账户名/商户名按统一语义契约判定；
/// 排序固定交易日期降序（created_at、id 兜底，防同秒批量写入翻页漂移）；
/// 返回当前页与命中总数。
///
/// 支持可选筛选（与关键字 AND 组合，全部可省略、单边可用）：
/// - `amount_min_cents` / `amount_max_cents`：金额区间（整数分，含边界；按本位币分
///   `amount_native_cents` 过滤，与全仓聚合口径同源，多币种下跨币种不再混滤）；
/// - `date_from` / `date_to`：日期区间（`YYYY-MM-DD` 字符串比较，含边界）。
///
/// 空查询（无关键字）时：有筛选 → 执行仅筛选查询；无筛选 → 维持返回空结果。
///
/// 参数较多（8 个）是 issue #40 规格要求的签名（四个可选筛选参数直传，BDD/单测
/// 沿用直调内部函数模式），故显式 allow `too_many_arguments`。
#[allow(clippy::too_many_arguments)]
pub fn search_transactions_internal(
    conn: &Connection,
    query: &str,
    page: usize,
    page_size: usize,
    amount_min_cents: Option<i64>,
    amount_max_cents: Option<i64>,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> Result<TransactionSearchResult> {
    let terms = split_terms(query);
    let has_filter = amount_min_cents.is_some()
        || amount_max_cents.is_some()
        || date_from.is_some()
        || date_to.is_some();
    // 空关键字 + 无筛选 → 空结果（既有语义）；空关键字 + 有筛选 → 仅筛选查询。
    if terms.is_empty() && !has_filter {
        return Ok(TransactionSearchResult {
            items: Vec::new(),
            total: 0,
        });
    }
    let page = page.max(1);
    let page_size = page_size.clamp(1, MAX_PAGE_SIZE);

    // 惰性回填存量行的拼音冗余列（V018）：与显式一键修复（issue #513）消费
    // 同一回填核心；报告在搜索路径不消费，失败已 warn，语义由现算兜底保障。
    backfill_note_pinyin(conn);

    // 可搜索名字与软删口径字典（账户/分类/商户，个位数到千行量级）：每次搜索
    // 新建，替代 50 万候选流上的逐行 JOIN（改名即刻生效语义不变，见模块注释）。
    let dicts = load_search_dicts(conn)?;

    // 交易行口径（is_deleted = 0）+ 可选金额/日期过滤（走既有 B-tree 索引）。
    let mut where_clauses: Vec<&str> = vec!["t.is_deleted = 0"];
    let mut params: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(min) = amount_min_cents {
        // 本位币分口径（issue #395）：与全仓聚合一致，多币种下跨币种不再混滤。
        where_clauses.push("t.amount_native_cents >= ?");
        params.push(min.into());
    }
    if let Some(max) = amount_max_cents {
        where_clauses.push("t.amount_native_cents <= ?");
        params.push(max.into());
    }
    if let Some(from) = date_from {
        where_clauses.push("t.date >= ?");
        params.push(from.to_string().into());
    }
    if let Some(to) = date_to {
        where_clauses.push("t.date <= ?");
        params.push(to.to_string().into());
    }

    // 第一段：单表最小列流式匹配（词条之间 AND；字段 OR 见 `term_matches_borrowed`）。
    // 词条一次小写化；行内文本借用匹配，命中且落当前页窗口才分配 id
    // （内存 O(当前页)）。
    let term_lowers: Vec<TermLowered> = terms
        .iter()
        .map(|t| TermLowered {
            lower: t.to_lowercase(),
        })
        .collect();
    let mut stmt = conn.prepare(&stage1_sql(&where_clauses, !terms.is_empty()))?;
    // saturating 运算防极端输入（usize::MAX）下溢/溢出 panic（与 list_transactions 先例一致）；
    // 超出命中数的页返回空页。
    let offset = page.saturating_sub(1).saturating_mul(page_size);
    let mut total: i64 = 0;
    let mut page_ids: Vec<String> = Vec::with_capacity(page_size);
    {
        let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
            // 列序与 [`stage1_sql`] 的 SELECT 清单一一对应；文本列经 get_ref
            // 借用（NULL → None），零分配匹配。
            let id = row.get_ref(0)?.as_str()?;
            let note = row.get_ref(1)?.as_str().ok();
            let note_pinyin = row.get_ref(2)?.as_str().ok();
            let account_id = row.get_ref(3)?.as_str()?;
            let merchant_id = row.get_ref(4)?.as_str().ok();
            let category_id = row.get_ref(5)?.as_str().ok();

            // 行级口径过滤（与原 JOIN 谓词等价）：账户必须在用；分类未软删（可空）。
            let Some((account, account_deleted)) = dicts.accounts.get(account_id) else {
                return Ok(());
            };
            if *account_deleted {
                return Ok(());
            }
            if let Some(cid) = category_id
                && dicts.categories.get(cid).copied().unwrap_or(false)
            {
                return Ok(());
            }
            let merchant = merchant_id.and_then(|mid| dicts.merchants.get(mid));
            if term_lowers
                .iter()
                .all(|t| term_matches_borrowed(t, note, note_pinyin, account, merchant))
            {
                total += 1;
                // 命中序号（0 起）落在当前页区间且未满页才收集 id。
                if total as usize > offset && page_ids.len() < page_size {
                    page_ids.push(id.to_string());
                }
            }
            Ok(())
        })?;
        for row in rows {
            row?;
        }
    }

    // 第二段：仅为当前页回表取展示列。
    let items = fetch_display_rows(conn, &page_ids)?;

    Ok(TransactionSearchResult { items, total })
}
