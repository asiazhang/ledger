//! 查询执行（读路径，issue #515 / ADR-0027 决策 1 修订）：搜索 SQL 下推权威。
//!
//! 职责：匹配段在 SQLite C 层完成（备注原文 LIKE + 账户/商户名字典 id 集合下推、
//! 软删口径一并下推，`INDEXED BY` 钉 V018 覆盖索引），展示段仅对当前页命中 id 回表
//! 18 列。不变量：交易搜索按原文命中（备注原文子串 ∨ 账户名 ∨ 商户名，词条间 AND、
//! 字段间 OR；拼音路径随 #1727 拼音退役整体拆除，下拉侧拼音可搜不受影响）。ADR 指针：
//! ADR-0027 决策 1 修订。陷阱：SQLite LIKE 大小写折叠仅 ASCII（非 ASCII 大写备注边界）。
use std::collections::HashMap;

use rusqlite::Connection;
use rusqlite::types::Value;

use crate::model::{Transaction, TransactionSearchResult};
use ledger_infra::db::query::FromRow;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;

use crate::shared::search_text::split_terms;

pub use search_transactions_internal as search_transactions;

/// 每页条数上限（防呆，防止极端输入拖垮查询）。
const MAX_PAGE_SIZE: usize = 200;

/// 已小写词条（搜索开始时一次性准备；下推 LIKE 模式由它派生）。
#[doc(hidden)]
pub struct TermLowered {
    pub lower: String,
}

/// 可搜索名字字典条目：小写化名字（搜索开始时算好，热路径免逐行分配）。
/// 名字即时读取语义由「字典每次搜索新建」保证（改名即刻生效）。
pub(super) struct DictEntry {
    pub(super) name_lower: String,
}

/// 搜索字典：可搜索名字与软删口径的小参考表，搜索开始时一次性读取。
/// - 账户：id → (条目, 是否软删)——账户名即时读取（改名即刻生效），软删账户
///   的交易不可搜（与 `JOIN accounts a ON … AND a.is_deleted = 0` 等价）；
/// - 分类：id → 是否软删——软删分类名下的交易不可搜（与
///   `(c.is_deleted = 0 OR c.id IS NULL)` 等价）；
/// - 商户：id → 条目（含软删商户——历史交易仍可搜，与既有 LEFT JOIN 等价，
///   无 is_deleted 过滤）。
#[doc(hidden)]
pub struct SearchDicts {
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

#[doc(hidden)]
pub fn load_search_dicts(conn: &Connection) -> Result<SearchDicts> {
    let entry = |name: String| DictEntry {
        name_lower: name.to_lowercase(),
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

/// 已小写词条对字典条目（账户/商户名）判定：原文连续子串（大小写不敏感由
/// 两侧小写化承担；名字体量小且字典每次搜索新建，改名即刻生效）。
fn term_matches_dict(term_lower: &str, entry: &DictEntry) -> bool {
    entry.name_lower.contains(term_lower)
}

/// 下推第一段查询：SQL 文本 + 绑定参数（`?N` 显式编号，与 `params` 下标一致）。
#[doc(hidden)]
pub struct Stage1Query {
    pub sql: String,
    pub params: Vec<Value>,
}

/// 可选筛选（金额/日期，与关键字 AND 组合）：列、比较符、绑定值。下推查询
/// 全语句统一用 `?N` 显式编号（不与匿名 `?` 混用，防编号漂移），由
/// [`build_stage1_query`] 按登记顺序分配；仅筛选路径（[`stage1_sql`]）沿用
/// 匿名 `?`（该语句无编号参数，无冲突）。
#[doc(hidden)]
pub struct Stage1Filter {
    pub column: &'static str,
    pub op: &'static str,
    pub value: Value,
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

/// 单词条下推子句（字段 OR）：备注原文子串 LIKE ∨ 账户名 ∨ 商户名。账户/商户
/// 侧由字典预判命中的 id 集合下推 IN（名字不固化在交易行上，即时读取语义由
/// 「字典每次搜索新建 + 集合现算」保持）；软删账户不进集合，与原行级口径过滤等价。
fn term_clause(term: &TermLowered, dicts: &SearchDicts, params: &mut Vec<Value>) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(3);
    params.push(like_substring_pattern(&term.lower).into());
    let note_pattern_index = params.len();
    parts.push(format!("t.note LIKE ?{note_pattern_index} ESCAPE '\\'"));
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
/// 全部进入 WHERE，`INDEXED BY` 钉定 V018 搜索覆盖索引——LIKE 匹配仍为全量
/// 扫描，钉定使排序由索引序满足（无临时 B-tree）且扫描 index-only（零回
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
///   AND (t.note LIKE ? ESCAPE '\\'
///        OR t.account_id IN (…) OR t.merchant_id IN (…))
///   AND …
///   [AND 金额/日期筛选]
/// ORDER BY t.date DESC, t.created_at DESC, t.id DESC
/// ```
#[doc(hidden)]
pub fn build_stage1_query(
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
        "SELECT t.id,t.note,t.account_id,t.merchant_id,t.category_id \
         FROM transactions t \
         WHERE {} \
         ORDER BY t.date DESC, t.created_at DESC, t.id DESC",
        where_clauses.join(" AND ")
    )
}

/// 第二段：仅为当前页命中 id 回表取展示列（`Transaction::from_row` 的 21 列，
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
         t.to_account_id,t.funding_account_id,t.category_id,t.refund_of_transaction_id,t.note,t.date,t.created_at,\
         t.updated_at,t.version,t.device_id,t.is_deleted,t.merchant_id,t.policy_id,t.fx_rate_used,t.fx_rate_source \
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
///
/// **读快照一致性（issue #1699）**：字典装载、命中计数、回表页与来源 / 转换投影
/// 填充是多语句读闭包，整体收进同一读事务（嵌套感知）——写提交落在语句之间会
/// 页与总数错位（与列表 `list_transactions_internal` 同形同修）。
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
    // 读快照（issue #1699）：字典、命中计数、回表页与投影填充收进同一读事务。
    ensure_transaction(conn, || {
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
                let account_id = row.get_ref(2)?.as_str()?;
                let category_id = row.get_ref(4)?.as_str().ok();

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
        crate::read::source::attach_sources(conn, &mut items)?;
        crate::read::source::attach_convert_fields(conn, &mut items)?;

        Ok(TransactionSearchResult { items, total })
    })
}
