//! 查询执行：SQL 下推（issue #515，修订 ADR-0027 决策 1）——匹配在 SQLite C 层
//! 完成，Rust 不再逐行扫描。
//!
//! 语义契约仍是 ADR-0027 统一模糊搜索规格（原文连续子串 ∨ 拼音首字母子序列，
//! 词条之间 AND、字段之间 OR，多音字前字规则），本模块只改**实现形态**：
//!
//! 1. **第一段（匹配段，SQL 下推）**：词条对备注（原文子串 `LIKE`、拼音首字母
//!    子序列逐字符 `%` 连接的多段 `LIKE`，严格等价于跳字子序列；词条含
//!    `[a-z0-9]` 之外字符时拼音分支必然失败、精确省去）与账户名/商户名（字典
//!    预判命中 id 集合下推 `IN`，复用既有纯函数语义）的匹配全部进入 WHERE；
//!    软删口径（交易行、软删账户 `NOT IN`、软删分类 `NOT IN`）一并下推。查询以
//!    `INDEXED BY` 钉定 V018 搜索覆盖索引 `idx_transactions_note_search`，
//!    index-only 全扫、零回表、列表序供序（无临时 B-tree，EXPLAIN 测试钉定），
//!    仅命中行的 id 流出 SQLite。账户/商户名即时读取与改名即刻生效语义由
//!    「字典每次搜索新建 + 命中 id 集合现算」保持；词条内的 `%`、`_`、`\`
//!    经 `ESCAPE '\'` 转义按字面匹配。**已知边界（显式记录，不兜底）**：
//!    SQLite LIKE 大小写折叠仅对 ASCII 生效，备注含非 ASCII 大写字母且用户
//!    以另一大小写搜索时不命中（账户/商户名字典路径仍在 Rust 侧全 Unicode
//!    折叠，不受影响）。
//! 2. **第二段（展示段）**：仅为当前页（≤ page_size）命中 id 回表取展示列
//!    （`Transaction::from_row` 的 18 列，无 JOIN）。
//! 3. **note_pinyin 兜底承诺修订**：搜索入口保留惰性自动回填（与显式一键修复
//!    [`repair_note_pinyin`] 同一实现，issue #513），**去掉运行时逐行现算兜底**
//!    ——回填失败时拼音路径降级漏配（warn），可经设置页一键修复恢复。
//!
//! 仅金额/日期筛选（无关键字）路径保持原形态：最小列流式扫描 + Rust 层口径
//! 过滤（planner 自由——筛选选择性依赖数据分布，量筛选索引可能更优）。
//!
//! 热路径纪律：命中且落当前页窗口才分配 id——下推后每行仅流出 id 列，内存
//! O(当前页 + 小参考字典)（流式计数、仅页 id 物化；字典体量随账户/分类/商户
//! 字典而非交易流水增长）。词条数与 IN 集合体量受 SQLite 变量上限约束
//! （默认 32766，现实输入远不可达）。

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

/// 已小写词条（搜索开始时一次性准备；下推 LIKE 模式由它派生）。
pub(super) struct TermLowered {
    pub(super) lower: String,
}

/// 可搜索名字字典条目：小写化名字 + 拼音首字母串（均搜索开始时算好，热路径
/// 免逐行分配）。名字即时读取语义由「字典每次搜索新建」保证（改名即刻生效）。
pub(super) struct DictEntry {
    pub(super) name_lower: String,
    pub(super) pinyin: String,
}

/// 搜索字典：可搜索名字与软删口径的小参考表，搜索开始时一次性读取。
/// - 账户：id → (条目, 是否软删)——账户名即时读取（改名即刻生效），软删账户
///   的交易不可搜（与 `JOIN accounts a ON … AND a.is_deleted = 0` 等价）；
/// - 分类：id → 是否软删——软删分类名下的交易不可搜（与
///   `(c.is_deleted = 0 OR c.id IS NULL)` 等价）；
/// - 商户：id → 条目（含软删商户——历史交易仍可搜，与既有 LEFT JOIN 等价，
///   无 is_deleted 过滤）。
pub(super) struct SearchDicts {
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

pub(super) fn load_search_dicts(conn: &Connection) -> Result<SearchDicts> {
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

/// LIKE 通配转义（配 `ESCAPE '\'`）：`\`→`\\`、`%`→`\%`、`_`→`\_`，其余字符
/// 原样。转义符只出现在这三类序列前——SQLite 对「转义符 + 非特殊字符」的序列
/// 按不匹配处理，故 `\` 自身也要翻倍，保证特殊字符按字面匹配。
fn escape_like(term: &str) -> String {
    let mut out = String::with_capacity(term.len() + 8);
    for ch in term.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// 原文连续子串 LIKE 模式：`%term%`（词条已小写；ASCII 大小写折叠由 LIKE
/// 自带，非 ASCII 边界见模块注释）。
fn like_substring_pattern(term_lower: &str) -> String {
    format!("%{}%", escape_like(term_lower))
}

/// 拼音首字母子序列 LIKE 模式：词条逐字符以 `%` 连接（`kf` → `%k%f%`），
/// 严格等价于「字符按原序出现、允许跳字」的子序列判定；字符同样经转义。
fn like_subsequence_pattern(term_lower: &str) -> String {
    let mut out = String::from("%");
    for ch in term_lower.chars() {
        out.push_str(&escape_like(&ch.to_string()));
        out.push('%');
    }
    out
}

/// 已小写词条对目标的子序列判定（热路径，字典预判侧仍在 Rust）。语义与
/// [`is_subsequence`] 一致（两侧小写化后逐字符有序匹配）：`pattern` 已小写；
/// `target` 无大写字符时小写化为恒等、直接免分配匹配，含大写（冷情形）降级
/// 为原分配路径。
fn is_subsequence_lower(pattern_lower: &str, target: &str) -> bool {
    if target.chars().any(char::is_uppercase) {
        return is_subsequence(pattern_lower, target);
    }
    let mut chars = target.chars();
    pattern_lower.chars().all(|p| chars.any(|t| t == p))
}

/// 已小写词条对字典条目（账户/商户名）判定：原文子串 ∨ 拼音首字母子序列。
/// 语义与 [`term_matches_text`](super::search_text::term_matches_text) 一致
/// （两侧均已小写化；名字体量小且字典每次搜索新建，改名即刻生效）。
fn term_matches_dict(term_lower: &str, entry: &DictEntry) -> bool {
    entry.name_lower.contains(term_lower) || is_subsequence_lower(term_lower, &entry.pinyin)
}

/// 下推第一段查询：SQL 文本 + 绑定参数（`?N` 显式编号，与 `params` 下标一致）。
pub(super) struct Stage1Query {
    pub(super) sql: String,
    pub(super) params: Vec<Value>,
}

/// 可选筛选（金额/日期，与关键字 AND 组合）：列、比较符、绑定值。下推查询
/// 全语句统一用 `?N` 显式编号（不与匿名 `?` 混用，防编号漂移），由
/// [`build_stage1_query`] 按登记顺序分配；仅筛选路径（[`stage1_sql`]）沿用
/// 匿名 `?`（该语句无编号参数，无冲突）。
pub(super) struct Stage1Filter {
    pub(super) column: &'static str,
    pub(super) op: &'static str,
    pub(super) value: Value,
}

/// IN 集合子句形态（占位符编号与参数登记统一收口此处，防三处同形拼装漂移）。
enum InClauseKind {
    /// `col IN (?,?,…)`——空集合恒假占位 `0`（无可命中 id）。
    In,
    /// `col NOT IN (?,?,…)`——空集合恒真占位 `1`（无排除对象，子句可省）。
    NotIn,
    /// `(col IS NULL OR col NOT IN (?,?,…))`——可空列不约束；空集合恒真占位 `1`。
    NullableNotIn,
}

/// IN 集合子句：非空集合按形态生成 SQL 并登记参数；空集合按语义生成恒假
/// （IN）或恒真（NOT IN 系）占位。
fn push_in_clause(
    col: &str,
    ids: Vec<Value>,
    params: &mut Vec<Value>,
    kind: InClauseKind,
) -> String {
    if ids.is_empty() {
        return match kind {
            InClauseKind::In => "0".into(),
            InClauseKind::NotIn | InClauseKind::NullableNotIn => "1".into(),
        };
    }
    let placeholders = ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", params.len() + i + 1))
        .collect::<Vec<_>>()
        .join(",");
    params.extend(ids);
    match kind {
        InClauseKind::In => format!("{col} IN ({placeholders})"),
        InClauseKind::NotIn => format!("{col} NOT IN ({placeholders})"),
        InClauseKind::NullableNotIn => {
            format!("({col} IS NULL OR {col} NOT IN ({placeholders}))")
        }
    }
}

/// 拼音分支可命中判定：`note_pinyin` 列只含 `[a-z0-9]`（拼音首字母规则：
/// ASCII 字母/数字小写保留、其余跳过），子序列要求词条每个字符都出现在列值中——
/// 词条含任一 `[a-z0-9]` 之外字符（汉字、标点、`%` 等）时拼音路径必然失败，
/// 该 LIKE 分支可精确省去（中文搜索成本减半，语义零变更）。
fn pinyin_branch_possible(term_lower: &str) -> bool {
    term_lower
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// 单词条下推子句（字段 OR）：备注原文子串 LIKE ∨ 拼音首字母子序列 LIKE（仅当
/// [`pinyin_branch_possible`]）∨ 账户名 ∨ 商户名。账户/商户侧由字典预判命中的
/// id 集合下推 IN（名字不固化在交易行上，即时读取语义由「字典每次搜索新建 +
/// 集合现算」保持）；软删账户不进集合，与原行级口径过滤等价。
fn term_clause(term: &TermLowered, dicts: &SearchDicts, params: &mut Vec<Value>) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(4);
    params.push(like_substring_pattern(&term.lower).into());
    let note_pattern_index = params.len();
    parts.push(format!("t.note LIKE ?{note_pattern_index} ESCAPE '\\'"));
    if pinyin_branch_possible(&term.lower) {
        params.push(like_subsequence_pattern(&term.lower).into());
        let pinyin_pattern_index = params.len();
        parts.push(format!(
            "t.note_pinyin LIKE ?{pinyin_pattern_index} ESCAPE '\\'"
        ));
    }
    let account_ids: Vec<Value> = dicts
        .accounts
        .iter()
        .filter(|(_, (entry, deleted))| !*deleted && term_matches_dict(&term.lower, entry))
        .map(|(id, _)| Value::Text(id.clone()))
        .collect();
    parts.push(push_in_clause(
        "t.account_id",
        account_ids,
        params,
        InClauseKind::In,
    ));
    let merchant_ids: Vec<Value> = dicts
        .merchants
        .iter()
        .filter(|(_, entry)| term_matches_dict(&term.lower, entry))
        .map(|(id, _)| Value::Text(id.clone()))
        .collect();
    parts.push(push_in_clause(
        "t.merchant_id",
        merchant_ids,
        params,
        InClauseKind::In,
    ));
    format!("({})", parts.join(" OR "))
}

/// 第一段下推查询（issue #515，修订 ADR-0027 决策 1）：词条匹配与软删口径
/// 全部进入 WHERE，`INDEXED BY` 钉定 V018 搜索覆盖索引——子序列语义决定全量
/// 扫描本质，钉定使排序由索引序满足（无临时 B-tree）且扫描 index-only（零回
/// 表），并防 planner 在统计边际上摇摆（先例：V016 月度表达式索引钉定）。
/// 账户/分类/商户侧口径由字典预判成 id 集合下推（50 万候选流上 JOIN 即取行
/// 主要成本，实测 25ms → 1170ms，V018 修订记录）。
///
/// SQL 形态（每词条一组字段 OR，词条之间 AND，金额/日期筛选以
/// [`Stage1Filter`] 描述、编号拼接在尾部）：
///
/// ```sql
/// SELECT t.id FROM transactions t INDEXED BY idx_transactions_note_search
/// WHERE t.is_deleted = 0
///   [AND t.account_id NOT IN (软删账户)]
///   AND (t.category_id IS NULL OR t.category_id NOT IN (软删分类))
///   AND (t.note LIKE ? ESCAPE '\\' OR t.note_pinyin LIKE ? ESCAPE '\\'
///        OR t.account_id IN (…) OR t.merchant_id IN (…))
///   AND …
///   [AND 金额/日期筛选]
/// ORDER BY t.date DESC, t.created_at DESC, t.id DESC
/// ```
pub(super) fn build_stage1_query(
    term_lowers: &[TermLowered],
    dicts: &SearchDicts,
    filters: &[Stage1Filter],
) -> Stage1Query {
    let mut params: Vec<Value> = Vec::new();
    let mut clauses: Vec<String> = Vec::with_capacity(term_lowers.len() + 4);
    clauses.push("t.is_deleted = 0".to_string());

    // 全局口径：软删账户名下的交易不可搜（与原行级过滤等价）。用
    // `NOT IN (软删账户集合)` 而非 `IN (在用全集)`——软删集合通常为空（子句
    // 整体省略、零每行开销），且外键强制下账户引用不可能悬空
    //（PRAGMA foreign_keys = ON），两种形态语义等价。
    let deleted_account_ids: Vec<Value> = dicts
        .accounts
        .iter()
        .filter(|(_, (_, deleted))| *deleted)
        .map(|(id, _)| Value::Text(id.clone()))
        .collect();
    if !deleted_account_ids.is_empty() {
        clauses.push(push_in_clause(
            "t.account_id",
            deleted_account_ids,
            &mut params,
            InClauseKind::NotIn,
        ));
    }

    // 软删分类名下的交易不可搜（分类可空不约束，与 (c.is_deleted=0 OR c.id IS NULL) 等价）。
    let deleted_category_ids: Vec<Value> = dicts
        .categories
        .iter()
        .filter(|(_, deleted)| **deleted)
        .map(|(id, _)| Value::Text(id.clone()))
        .collect();
    clauses.push(push_in_clause(
        "t.category_id",
        deleted_category_ids,
        &mut params,
        InClauseKind::NullableNotIn,
    ));

    // 词条 AND：每词条一组字段 OR（见 [`term_clause`]）。
    for term in term_lowers {
        clauses.push(term_clause(term, dicts, &mut params));
    }

    // 可选金额/日期筛选（与关键字 AND 组合，编号与登记顺序一致）。
    for f in filters {
        params.push(f.value.clone());
        clauses.push(format!("{} {} ?{}", f.column, f.op, params.len()));
    }

    Stage1Query {
        sql: format!(
            "SELECT t.id FROM transactions t INDEXED BY idx_transactions_note_search \
             WHERE {} \
             ORDER BY t.date DESC, t.created_at DESC, t.id DESC",
            clauses.join(" AND ")
        ),
        params,
    }
}

/// 仅金额/日期筛选路径（无关键字）的第一段 SQL：单表最小列候选流（不物化展示
/// 列、无 JOIN），软删除交易口径与可选金额/日期过滤与交易列表一致，按交易日期
/// 降序（created_at、id 兜底，防同秒批量写入翻页漂移）预排序。不钉定索引——
/// 筛选选择性依赖数据分布，planner 自由（量筛选索引可能更优）；软删账户/分类
/// 口径由 [`SearchDicts`] 在 Rust 层逐行判定（与关键字路径同一字典）。
pub(super) fn stage1_sql(where_clauses: &[&str]) -> String {
    format!(
        "SELECT t.id,t.note,t.note_pinyin,t.account_id,t.merchant_id,t.category_id \
         FROM transactions t \
         WHERE {} \
         ORDER BY t.date DESC, t.created_at DESC, t.id DESC",
        where_clauses.join(" AND ")
    )
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
/// 派生数据维护不置脏（不触发备份）。搜索入口的惰性回填消费同一核心
/// [`backfill_note_pinyin`]（报告在搜索路径消费收敛位，失败已 warn），本入口
/// 只以「一键修复」领域语言显式触发同一实现并取报告。
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
    // 同一回填核心。兜底承诺修订（issue #515 / ADR-0027 修订）：不再逐行现算
    // 兜底——回填失败时拼音路径降级漏配（warn），可经设置页一键修复恢复。
    let repair_report = backfill_note_pinyin(conn);
    if !repair_report.converged {
        tracing::warn!(
            backfilled = repair_report.backfilled,
            "备注拼音列仍有积压，拼音子序列路径可能漏配，可在设置中一键修复"
        );
    }

    // 可搜索名字与软删口径字典（账户/分类/商户，个位数到千行量级）：每次搜索
    // 新建，替代 50 万候选流上的逐行 JOIN（改名即刻生效语义不变，见模块注释）。
    let dicts = load_search_dicts(conn)?;

    // 可选金额/日期过滤（走既有 B-tree 索引；与关键字 AND 组合）。
    let mut filters: Vec<Stage1Filter> = Vec::new();
    if let Some(min) = amount_min_cents {
        // 本位币分口径（issue #395）：与全仓聚合一致，多币种下跨币种不再混滤。
        filters.push(Stage1Filter {
            column: "t.amount_native_cents",
            op: ">=",
            value: min.into(),
        });
    }
    if let Some(max) = amount_max_cents {
        filters.push(Stage1Filter {
            column: "t.amount_native_cents",
            op: "<=",
            value: max.into(),
        });
    }
    if let Some(from) = date_from {
        filters.push(Stage1Filter {
            column: "t.date",
            op: ">=",
            value: from.to_string().into(),
        });
    }
    if let Some(to) = date_to {
        filters.push(Stage1Filter {
            column: "t.date",
            op: "<=",
            value: to.to_string().into(),
        });
    }

    // saturating 运算防极端输入（usize::MAX）下溢/溢出 panic（与 list_transactions 先例一致）；
    // 超出命中数的页返回空页。
    let offset = page.saturating_sub(1).saturating_mul(page_size);
    let mut total: i64 = 0;
    let mut page_ids: Vec<String> = Vec::with_capacity(page_size);
    if terms.is_empty() {
        // 仅筛选路径：最小列流式扫描 + Rust 层口径过滤（planner 自由，见
        // [`stage1_sql`]）。行内文本经 get_ref 借用（NULL → None），零分配。
        let mut where_clauses: Vec<String> = vec!["t.is_deleted = 0".to_string()];
        where_clauses.extend(filters.iter().map(|f| format!("{} {} ?", f.column, f.op)));
        let where_refs: Vec<&str> = where_clauses.iter().map(String::as_str).collect();
        let filter_params: Vec<Value> = filters.iter().map(|f| f.value.clone()).collect();
        let mut stmt = conn.prepare(&stage1_sql(&where_refs))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(filter_params.iter()), |row| {
            // 列序与 [`stage1_sql`] 的 SELECT 清单一一对应。
            let id = row.get_ref(0)?.as_str()?;
            let account_id = row.get_ref(3)?.as_str()?;
            let category_id = row.get_ref(5)?.as_str().ok();

            // 行级口径过滤（与原 JOIN 谓词等价）：账户必须在用；分类未软删（可空）。
            let Some((_, account_deleted)) = dicts.accounts.get(account_id) else {
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
            total += 1;
            // 命中序号（0 起）落在当前页区间且未满页才收集 id。
            if total as usize > offset && page_ids.len() < page_size {
                page_ids.push(id.to_string());
            }
            Ok(())
        })?;
        for row in rows {
            row?;
        }
    } else {
        // 关键字路径：SQL 下推（issue #515，见 [`build_stage1_query`]）。匹配在
        // SQLite C 层完成，仅命中行的 id 流出，命中计数 total、仅收集当前页 id
        // （内存 O(当前页)）。
        let term_lowers: Vec<TermLowered> = terms
            .iter()
            .map(|t| TermLowered {
                lower: t.to_lowercase(),
            })
            .collect();
        let query = build_stage1_query(&term_lowers, &dicts, &filters);
        let mut stmt = conn.prepare(&query.sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(query.params.iter()), |row| {
            let id = row.get_ref(0)?.as_str()?;
            total += 1;
            // 命中序号（0 起）落在当前页区间且未满页才收集 id。
            if total as usize > offset && page_ids.len() < page_size {
                page_ids.push(id.to_string());
            }
            Ok(())
        })?;
        for row in rows {
            row?;
        }
    }

    // 第二段：仅为当前页回表取展示列；来源列随页填充（与列表命令同一反查，
    // spec #704 / issue #706：搜索页与交易页同一来源口径）。
    let mut items = fetch_display_rows(conn, &page_ids)?;
    super::read::attach_sources(conn, &mut items)?;

    Ok(TransactionSearchResult { items, total })
}
