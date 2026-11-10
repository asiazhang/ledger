//! 域栈引导（issue #2017 / ADR-0145）：打开未加密账本库，读交易列表页、交易
//! 搜索与参考数据（币种字典 / 在用分类 / 可见账户 / 含软删商户——交易表格
//! 首屏、issue #2023 搜索接入与派生行形态的装配消费面），#2033 起兼预算
//! 读写、#2034 起兼交易表单族参考数据与基础收支写路径、#2053 起兼设置接缝
//! （日志档位 / DataLocation 信息聚合与更改意图 / 引导记录）、#2045 起兼投
//! 资域读接缝（投资概览/盈亏页签与走势/收益率取数）。
//!
//! 引导内核复用基础设施引导层（[`ledger_infra::db::boot`]）：库文件处置判定
//! （[`classify_for_boot`] 的明文/外来形态/密文/损坏四态）与成对建连
//! （[`open_db_in`]：写连接迁移先行 + 只读读连接，ADR-0117 决策 3）都是既有
//! 单点，本模块只做壳侧编排——与旧壳 `boot_sequence` 的明文路径同形（含外来
//! 形态明文库建连前归一化，issue #1453），不另立第二份判定。
//!
//! 立桩收窄（spec #2015「对未加密账本库引导启动，无解锁向导」）：
//! - 密文库（AwaitUnlock）→ 码化错误 [`CODE_ENCRYPTED_UNSUPPORTED`]（解锁向导
//!   属旧壳能力，原生壳随迁移系列票补齐）；
//! - 库文件不存在（[`DB_FILE_NAME`] 缺席）→ 码化错误 [`CODE_BOOK_MISSING`]：
//!   启动参数是显式账本目录，静默新建空库会把路径打错吞成「交易总数 0 条」，
//!   fail loud 优于假空态；
//! - 损坏残留（Unreadable）→ 沿用基础设施 [`BOOT_DB_UNREADABLE`] 码与文案
//!   （与旧壳同码同文案，用户可见语义一致）。
//!
//! 界面文案仅 zh-CN（spec #2015 实施决策：i18n 框架级方案不在立桩票）；码化
//! 错误的中文原样显示（NSH 壳无错误模板同步面——旧壳错误模板服务旧前端，本壳
//! 消息随码走，i18n 方案票再收敛）。
//!
//! 写路径收口（issue #2087）：写函数族（交易 / 预算 / 账户 / 物品 / 商户 /
//! 计划 / 储蓄目标 / 保单 / 保司 / 实物资产 / 投资手动写）一律经
//! [`crate::shell_support::write_entry_locked`]（壳层统一写入口直锁形态）——
//! 连接层写入口提交点钩子（备份域置脏 + 写时顺带到期检查，ADR-0032）由此
//! 结构性生效；豁免与旧壳同口径（设置 KV 写、备份快照、恢复路径、同步编排
//! 段接缝不经写入口）。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use ledger_categories::Category;
use ledger_currencies::Currency;
use ledger_infra::db::boot::{BOOT_DB_UNREADABLE, BootDisposition};
use ledger_infra::db::data_location::DB_FILE_NAME;
use ledger_infra::db::{DbState, open_db_in};
use ledger_infra::error::{AppError, Result};
use ledger_transaction::{TransactionListResult, TransactionSearchResult};

use crate::assembly::PAGE_SIZE;
use crate::shell_support::write_entry_locked;
use crate::{logging, wiring};

/// 密文库不支持（立桩范围外）的稳定错误码。
pub const CODE_ENCRYPTED_UNSUPPORTED: &str = "nsh-shell.encrypted-unsupported";

/// 账本库文件缺失的稳定错误码。
pub const CODE_BOOK_MISSING: &str = "nsh-shell.book-missing";

/// 账本库会话缺失的稳定错误码（写路径在会话就位前执行的不可达兜底）。
pub const CODE_SESSION_MISSING: &str = "nsh-shell.session-missing";

/// 文件保存对话框无法打开的稳定错误码（#2038：Linux 无文件选择 portal 等
/// 平台能力缺失；平台错误原文随文案透传，呈现面归视图层）。
pub const CODE_FILE_DIALOG_FAILED: &str = "nsh-shell.file-dialog-failed";

/// 日志档位非法（闭集外值）的稳定错误码（旧壳 `set_log_level` 命令同码，
/// [`shell_support::logger::set_persisted_level`] 的错误模板；本壳同码同文案，
/// 用户可见语义一致）。
pub const CODE_LOG_LEVEL_INVALID: &str = "settings.log-level-invalid";

/// 外来形态归一化失败的稳定错误码（与旧壳 `boot.db-normalize-failed` 同码，
/// 基础设施 [`ledger_infra::db::boot::BOOT_DB_NORMALIZE_FAILED`]）。
const BOOT_DB_NORMALIZE_FAILED: &str = ledger_infra::db::boot::BOOT_DB_NORMALIZE_FAILED;

/// 读路径共用的取锁守卫（七个读函数的 conn-lock 前奏收进单点）：读连接锁
/// 中毒统一映射为码化 [`AppError::Db`]。泛型不点名连接类型——本壳不直依赖
/// rusqlite，连接类型随 [`DbState`] 字段推断。
fn conn_lock<T>(mutex: &Mutex<T>) -> ledger_infra::error::Result<MutexGuard<'_, T>> {
    mutex.lock().map_err(|e| AppError::Db(e.to_string()))
}

/// 引导域栈打开未加密账本库（spec #2015：启动参数给账本目录，无解锁向导）。
///
/// 序列与旧壳明文启动路径同形：接缝接线（先于任何建库，幂等）→
/// [`ledger_infra::db::boot::plan_boot`] 引导记账世界（#2053 起：消费账本
/// 注册表，搬迁意图在下次启动生效；未配置及出厂世界与实参目录一一对应）→
/// 有效性预判（库文件缺席 fail loud）→ 处置判定 →（外来形态明文库先归
/// 一化，原库失败保持原样）→ 成对建连（迁移 + 只读连接 + 同步元数据库挂
/// 载，全部收在 [`open_db_in`] 单点）→ 引导记录入会话槽（设置卡数据位置
/// 面的读源）。
static BOOT_RECORD: Mutex<Option<ledger_infra::db::data_location::Boot>> = Mutex::new(None);

/// 引导记录槽位守卫（毒化时取回内值：槽位本身不含不变量）。
fn boot_record_slot() -> MutexGuard<'static, Option<ledger_infra::db::data_location::Boot>> {
    BOOT_RECORD.lock().unwrap_or_else(|p| p.into_inner())
}

/// 引导记录会话快照（设置卡聚合用）；未登记（极端时序）回 None。
#[doc(hidden)]
pub fn boot_record() -> Option<ledger_infra::db::data_location::Boot> {
    boot_record_slot().clone()
}

/// 复位引导记录槽（headless 测试接缝，[`crate::app::reset_book_assembly`] 消费）。
#[doc(hidden)]
pub fn reset_boot_record_for_tests() {
    *boot_record_slot() = None;
}

/// 启动读路径共用的启动会话锚（argv 显式臂）：本臂的「默认数据目录」 =
/// 启动参数账本目录（立桩契约的延续：身份即目录，目录即账本世界入口）；
/// #2053 起 [`ledger_infra::db::boot::plan_boot`] 在其上消费账本注册表——
/// DataLocation 更改意图（搬迁 / 接管）在下次启动（同目录参数）生效，与
/// 设置卡写出的意图同接缝（旧壳 plan_boot 消费面同形）。解析链总序
///（argv 显式 > 注册表活动指针 > 默认数据目录）见 [`boot_ledger_via_registry`]。
/// 库文件缺失 fail loud（决策 4 保留的内核，防假空态）。
pub fn boot_ledger(book_dir: &Path) -> Result<DbState> {
    wiring::install_domain_seams();
    let plan = ledger_infra::db::boot::plan_boot(book_dir);
    let db_path = plan.boot.db_dir.join(DB_FILE_NAME);
    if !db_path.is_file() {
        return Err(book_missing_error(&db_path));
    }
    boot_ledger_by_plan(plan)
}

/// 启动解析链缺参入口（ADR-0146 决策 1）：以数据根为注册表根引导——账本
/// 注册表活动指针 > 默认数据目录（argv 显式臂是 [`boot_ledger`]，优先级
/// 由调用方在缺参之前分流，不经本函数）。返回 `Ok(None)` = 解析落点即默认
/// 数据目录且库文件缺失（未配置无库 / 注册表损坏回退后无库——首启引导
/// 现场，呈现面据此呈现引导态，ADR-0146 决策 2）；注册表已解析活动本缺库
///（落点非默认目录）不在其列，仍走 book-missing fail loud（决策 4）。
/// 「argv 打错」与「注册表落点无库」的文案与失败态出口区分归 T4 重写，
/// 本票两臂同文案。引导现场的引导计划随返回落槽（含回退原因
/// `fallback_reason`），呈现面经 [`boot_record`] 消费（引导态损坏告示与
/// 现场 D 的就绪回退告示同源）。
pub fn boot_ledger_via_registry(default_dir: &Path) -> Result<Option<DbState>> {
    wiring::install_domain_seams();
    let plan = ledger_infra::db::boot::plan_boot(default_dir);
    let db_path = plan.boot.db_dir.join(DB_FILE_NAME);
    if !db_path.is_file() {
        if plan.boot.db_dir == default_dir {
            // 解析落点即默认数据目录且无库：未配置 / 损坏回退后无库——
            // 首启引导现场（ADR-0146 决策 2），不 fail loud 也不静默新建；
            // 引导计划落槽（回退原因随槽，呈现面经 boot_record 消费——
            // 与就绪分支的回退告示同源同款式）。
            *boot_record_slot() = Some(plan.boot);
            return Ok(None);
        }
        return Err(book_missing_error(&db_path));
    }
    boot_ledger_by_plan(plan).map(Some)
}

/// 首启新建账本核心（ADR-0146 决策 2/5，spec #2090 T2 #2092）：在指定目录
/// **建库 + 登记「默认账本」条目 + 置活动指针**（即时落盘），三步全部复用
/// 既有基础设施入口，不新建造库 / 登记逻辑：
/// 1. 建库 = [`open_db_in`]（缺文件建连即建全新库，迁移收口在写连接建连
///    路径内——绕开缺库报错、不绕开迁移）；
/// 2. 登记 + 活动指针 = `write_registry(single_default(dir))`——首本直落
///    目录本身（决策 5：与 infra 单本默认语义、词汇表「存量目录折叠为唯一
///    默认账本」升级路径一致；非 `books/<uuid>` 子目录形态），原子写即
///    即时落盘，活动指针随 `single_default` 同条目就位。
///
/// 注册表损坏现场（引导回退后无库的告示现场）：损坏指针文件被显式确认的
/// 新建覆盖——告示与确认对话（明示完整路径）已提供知情，且结果与出厂逃生
/// 舱（「恢复默认位置」折叠语义）同向：默认目录成为唯一默认账本；在册库
/// 文件不受影响（本函数不触碰任何既有库文件）。
///
/// 复用面：首启引导「新建账本」（本票）与后续选择器现场的无库新建腿
///（ADR-0146 决策 3 ③ / 决策 4 出口）共享本核心。会话开启不在本函数内——
/// 调用方经各自解析链臂开启（首启确认后重走启动竖井，本次启动即进主
/// 界面）。
pub fn create_default_book(dir: &Path) -> Result<()> {
    wiring::install_domain_seams();
    std::fs::create_dir_all(dir)?;
    open_db_in(dir)?;
    ledger_infra::db::book_registry::write_registry(
        dir,
        &ledger_infra::db::book_registry::BookRegistry::single_default(dir),
    )
}

/// 库文件缺失的码化错误（fail loud 单点；argv 打错与注册表落点无库两臂
/// 共用同码同文案，重写归 T4）。
fn book_missing_error(db_path: &Path) -> AppError {
    AppError::codedp(
        CODE_BOOK_MISSING,
        format!(
            "账本库文件不存在：{}（立桩期请传入已有账本的目录，不新建账本）",
            db_path.display()
        ),
        &[&db_path.to_string_lossy()],
    )
}

/// 引导计划的处置执行（argv 臂与解析链臂共用）：库文件有效性已由调用方
/// 预判，此处四态分流——明文直开 / 外来形态先归一化 / 密文与损坏残留码化
/// 失败；成功时引导记录落槽（设置卡数据位置面的读源，回退告示的呈现源）。
fn boot_ledger_by_plan(plan: ledger_infra::db::boot::BootPlan) -> Result<DbState> {
    let disposition = plan.disposition.map_err(|e| {
        tracing::error!(error = %e, "入库前处置探测失败");
        e
    })?;
    match disposition {
        BootDisposition::OpenPlaintext => {
            *boot_record_slot() = Some(plan.boot.clone());
            open_db_in(&plan.boot.db_dir)
        }
        BootDisposition::NormalizePlaintext => {
            normalize_foreign_plaintext(&plan.boot.db_dir.join(DB_FILE_NAME))?;
            *boot_record_slot() = Some(plan.boot.clone());
            open_db_in(&plan.boot.db_dir)
        }
        BootDisposition::AwaitUnlock => Err(AppError::coded(
            CODE_ENCRYPTED_UNSUPPORTED,
            "密文库暂不支持：原生壳立桩只引导未加密账本库，解锁向导随迁移系列票补齐",
        )),
        BootDisposition::Unreadable => Err(AppError::coded(
            BOOT_DB_UNREADABLE,
            "数据库文件无法打开（文件可能已损坏）",
        )),
    }
}

/// 外来形态明文库归一化（issue #1453）：失败映射为码化错误、原库保持原样——
/// 与旧壳 `boot_sequence` 的同段同码同文案（本壳无失败恢复屏，窗口直接呈现）。
fn normalize_foreign_plaintext(db_path: &Path) -> Result<()> {
    ledger_infra::db::encryption::normalize_plaintext_db_file(db_path).map_err(|e| {
        tracing::error!(error = %e, "外来形态明文库归一化失败，原库保持原样");
        AppError::coded(
            BOOT_DB_NORMALIZE_FAILED,
            "账本库文件形态异常，无法自动修复；请重启应用重试，或从备份文件恢复",
        )
    })
}

/// 交易列表页查询载荷（issue #2037 筛选读接缝）：筛选会话态快照的读路径投影。
/// 默认构造 = 无筛选但显式携带四通用 kind（主列表排除投资 kind 的读接缝单点，
/// ADR-0135——本壳立桩期未接筛选维度的缺口由此闭合）。
#[derive(Clone, Debug)]
pub struct TxListQuery {
    /// 期间快照（「全部」= `None`）。
    pub range: Option<crate::period::PeriodRange>,
    /// 类型集合（显式携带，含默认态；空集合视为未携带不过滤）。
    pub kinds: Vec<ledger_transaction::TransactionKind>,
    /// 涉及账户（转出 ∪ 转入 ∪ 出资三端命中，issue #937）。
    pub involving_account_id: Option<String>,
    /// 商户（含软删商户历史交易口径，issue #191）。
    pub merchant_id: Option<String>,
    /// 分类精确过滤（issue #377）。
    pub category_id: Option<String>,
    /// 仅无分类（issue #377 三态之一）。
    pub uncategorized_only: bool,
    /// 隐藏投资相关流水（#2062 / ADR-0136 视图偏好在读路径上的载荷，不是筛
    /// 选维度）：`true` 时行集排除转出 / 转入 / 出资三端任一端为投资类型账户
    /// 的行（域读路径三端 SQL 直传，按账户不按分类）；筛选会话态不携带，
    /// 由偏好（[`crate::tx_view_preferences`]）在装载时机写入。默认 `false`
    /// = 行为不变（域契约缺省不携带，只增）。
    pub hide_investment_related: bool,
}

impl Default for TxListQuery {
    fn default() -> Self {
        Self {
            range: None,
            kinds: crate::tx_filter::MAIN_LIST_KINDS.to_vec(),
            involving_account_id: None,
            merchant_id: None,
            category_id: None,
            uncategorized_only: false,
            hide_investment_related: false,
        }
    }
}

/// 读取账本库交易列表页（读路径直调域服务，issue #2018 立桩靶标；issue #2021
/// 补日期区间维度；issue #2037 补全筛选维度与默认 kind 收窄）。
///
/// 经核心交易域读路径 [`ledger_transaction::list_transactions`] 的服务端 offset
/// 分页（ADR-0008 语义：页码从 1 起、`total` 与行集共用同一 WHERE 子句、排序
/// date DESC, created_at DESC, id DESC），每页 [`PAGE_SIZE`] 行（100 行/页，
/// 旧壳 50 行翻倍验证原生余量）；`query.range` 为期间快照（含边界，自然周期
/// 换算产物见 [`crate::period`]），None = 「全部」无过滤；其余维度与类型集合
/// 按域契约直传（维度间 AND、类型维度内取或）。来源列等读时投影接缝已由
/// [`crate::wiring`] 接线。读连接是成对建连的只读侧（ADR-0117：读路径不进
/// 写者闸门）。
pub fn transaction_page(
    db: &DbState,
    page: u32,
    query: &TxListQuery,
) -> Result<TransactionListResult> {
    let filter = ledger_transaction::TransactionListFilter {
        page: Some(page.max(1) as usize),
        page_size: Some(PAGE_SIZE),
        from: query.range.as_ref().map(|r| r.from.clone()),
        to: query.range.as_ref().map(|r| r.to.clone()),
        involving_account_id: query.involving_account_id.clone(),
        merchant_id: query.merchant_id.clone(),
        category_id: query.category_id.clone(),
        uncategorized_only: query.uncategorized_only.then_some(true),
        kinds: (!query.kinds.is_empty()).then(|| query.kinds.clone()),
        hide_investment_related: query.hide_investment_related.then_some(true),
        ..Default::default()
    };
    let conn = conn_lock(&db.read_conn)?;
    ledger_transaction::list_transactions(&conn, &filter)
}

/// 搜索账本库交易（读路径直调域服务，issue #2023）：经核心交易域统一模糊
/// 搜索 [`ledger_transaction::search_transactions`]——词条间 AND、字段间 OR 的
/// 原文命中（备注原文子串 ∨ 账户名 ∨ 商户名 ∨ 购买项名称，LIKE 折叠仅 ASCII），
/// 服务端 offset 分页（命中总数与页行集共用同一 WHERE，排序 date DESC）。
/// 与旧壳 `search_transactions` 命令消费同一域函数——同一账本库同结果（#2023
/// AC1）；issue #2037 起接金额区间（整数分，本位币口径）与期间快照（搜索 ×
/// 期间裁决：期间对搜索命中面生效，旧壳搜索页同口径）；账户/商户/分类/类型
/// 维度不在搜索面（域读路径不支持，裁决定型为规格）。页大小沿 [`PAGE_SIZE`]
/// （分页页径与列表态一致）。空查询无筛选的域语义是空结果；空查询 + 有筛选
/// = 仅筛选查询（旧壳搜索页同语义，呈现态归视图层）。读连接是成对建连的
/// 只读侧（ADR-0117）。
pub fn transaction_search(
    db: &DbState,
    query: &str,
    page: u32,
    amount_min_cents: Option<i64>,
    amount_max_cents: Option<i64>,
    range: Option<&crate::period::PeriodRange>,
) -> Result<TransactionSearchResult> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_transaction::search_transactions(
        &conn,
        query,
        page.max(1) as usize,
        PAGE_SIZE,
        amount_min_cents,
        amount_max_cents,
        range.map(|r| r.from.as_str()),
        range.map(|r| r.to.as_str()),
    )
}

/// 删除交易（域公开写入口直调，issue #2022 写路径立桩靶标；不经 IPC）：
/// 经核心交易域行为层删除编排入口 [`ledger_transaction::delete`]——软删编排
/// （持仓副作用回退 + 级联 + 主行软删）同处一个事务，余额重算由已接线的余额
/// 刷新接缝（[`crate::wiring`]）在既有事务内执行。写经壳层统一写入口直锁
/// 形态（[`crate::shell_support::write_entry_locked`]，提交点置脏，#2087）。
/// 不存在的 id 返回码化 NotFound（并发已删的呈现面归视图层）。
pub fn delete_transaction(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_transaction::delete(conn, id))
}

// ---------------------------------------------------------------------------
// 物品视图读写接缝（#2044）：物品清单 / 单件成本与写路径（创建 / 修改 / 处置 /
// 软删）直调 `ledger-item` 域入口——与旧壳 list_items / create_item 等命令消费
// 费的同一域函数，不经 IPC。物品写路径在域内自持事务（溯源守卫 / 折算 / op
// 产出），本接缝零信号副作用——写成功后由视图广播 [`crate::signals`] 交易
// 失效信号（旧壳 ledger:changed 粗粒度语义的壳内对应物：交易列表来源列展示
// 物品名 / 处置态，物品变化经列表重读自然覆盖；广播接线删除即红钉在
// headless 名字改后来源列重读旅程）。

/// 关联购买交易候选页大小（旧壳 listTransactions limit 100 同值）。
const ITEM_CANDIDATE_LIMIT: usize = 100;

/// 清单物品行集（旧壳 `list_items` 命令同一执行协议）：每件附已用天数与每天
/// 成本快照（`item::cost` 接缝在域内计算，本壳只消费不另立口径；目标日在用
/// = 今天，已处置 = 处置日）。读连接是成对建连的只读侧（ADR-0117）。
pub fn item_rows(db: &DbState) -> Result<Vec<ledger_item::ItemWithDailyCost>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_item::list_items(&conn)
}

/// 按参考日重算单件物品每天成本（旧壳 `calculate_item_cost` 命令同一执行协
/// 议）：缺省 = 板用于清单同一口径（在用 → 今天 / 已处置 → 处置日），提供参
/// 考日即覆盖目标日（口径全在 `item::cost` 接缝单点）。读连接是成对建连的
/// 只读侧（ADR-0117）。
pub fn item_cost(
    db: &DbState,
    id: &str,
    reference_date: Option<&str>,
) -> Result<ledger_item::ItemDailyCost> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_item::calculate_item_cost(&conn, id, reference_date)
}

/// 创建物品（域公开写入口直调，旧壳 `create_item` 命令同一执行协议）：溯源守
/// 卫（创建必带购买交易指针）/ 带出 / 折算 / op 产出在域内单正文。写连接是
/// 成对建连的写侧。
pub fn create_item(db: &DbState, input: ledger_item::ItemInput) -> Result<String> {
    write_entry_locked(db, |conn| ledger_item::create_item(conn, input, &mut || {}))
}

/// 按 id 修改物品（域公开写入口直调，旧壳 `update_item` 命令同一执行协议）：
/// 换关带出 / 不可达状态守卫在域内。写连接是成对建连的写侧，写经统一写入口
/// 直锁形态（提交点置脏，issue #2087）。
pub fn update_item(db: &DbState, id: &str, input: ledger_item::ItemInput) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_item::update_item(conn, id, input, &mut || {})
    })
}

/// 处置物品（域公开写入口直调，旧壳 `dispose_item` 命令同一执行协议）：日期
/// / 残值守卫在域内；已处置再次处置 = 修正处置信息。写连接是成对建连的写侧。
pub fn dispose_item(db: &DbState, id: &str, input: ledger_item::ItemDisposeInput) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_item::dispose_item(conn, id, input, &mut || {})
    })
}

/// 软删物品（域公开写入口直调，旧壳 `delete_item` 命令同一执行协议）。写连
/// 接是成对建连的写侧。
pub fn delete_item(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_item::delete_item(conn, id, &mut || {}))
}

/// 关联购买交易候选（旧壳 ItemsView onMounted `listTransactions({ kinds:
/// ["expense"], limit: 100 })` 同口径）：支出交易倒序最近 100 笔，编辑弹窗
/// 换关下拉的数据源（更早的不在候选内，旧壳 MVP 取舍同口径；加载失败不阻断
/// 编辑弹窗，视图层容错）。读连接是成对建连的只读侧（ADR-0117）。
pub fn item_candidate_transactions(db: &DbState) -> Result<Vec<ledger_transaction::Transaction>> {
    let conn = conn_lock(&db.read_conn)?;
    let result = ledger_transaction::list_transactions(
        &conn,
        &ledger_transaction::TransactionListFilter {
            page: Some(1),
            page_size: Some(ITEM_CANDIDATE_LIMIT),
            kinds: Some(vec![ledger_transaction::TransactionKind::Expense]),
            ..Default::default()
        },
    )?;
    Ok(result.items)
}

/// 本位币基准（旧壳 `getBaseCurrency` 命令同一域来源）：「本位币折算」行
/// 的符号与后缀读账本本位币，读取失败由视图回退缺省 CNY（旧壳同口径软容
/// 错）。读连接是成对建连的只读侧（ADR-0117）。
pub fn base_currency_code(db: &DbState) -> Result<String> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_currencies::current_base_currency(&conn)
}

// 备份最小入口读写接缝（#2038）：手动备份与账本指纹直调 `ledger-backup` 域
// 入口——与旧壳 `create_backup` / `get_book_fingerprint` 命令消费的同一域函数，
// 不经 IPC。
// ---------------------------------------------------------------------------

/// 创建手动备份（域 backup 直调，旧壳 `create_backup` 命令同一执行协议）：
/// 目标先经域受管改道判定（[`ledger_backup::managed_command_target`]——落在
/// 配置的备份目录内且为受管命名时改道设备子目录；NSH 本票无备份目录设置
/// 面、进程内偏好镜像未配置时原样落盘，与旧壳未配置备份目录的另存为同
/// 口径），再 `VACUUM INTO` 一致性快照打包 zip（ADR-0007 文件级语义，双库
/// 成对 ADR-0139）。写连接锁内裸作业：备份是只读快照，不经统一写入口的置
/// 脏/到期检查（与旧壳命令同口径，连接锁预算见备份域 [`ledger_backup::LOCK_HOLD_BUDGET`]）。
pub fn create_backup(
    db: &DbState,
    target: &Path,
    app_version: &str,
) -> Result<ledger_backup::BackupResult> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let target = ledger_backup::managed_command_target(target, &conn)?;
    ledger_backup::backup_db_to(
        &conn,
        &target,
        app_version,
        ledger_backup::BackupKind::Manual,
    )
}

/// 当前账本指纹只读判定（备份默认文件名的标识段，旧壳 `get_book_fingerprint`
/// 命令消费的同一域函数）：已固化恒读固化值，未固化回内容锚，空库且未固化
/// = `None`（退化为无标识旧命名，产物归属由内层元数据水印承担）。读失败归
/// `None`——旧壳 `refreshBookFingerprint` 容错同口径：指纹缺失只退化命名，
/// 不阻断备份。读连接是成对建连的只读侧（ADR-0117）。
pub fn book_fingerprint(db: &DbState) -> Option<String> {
    let conn = conn_lock(&db.read_conn).ok()?;
    ledger_backup::book_fingerprint_readonly(&conn)
        .ok()
        .flatten()
}

// ---------------------------------------------------------------------------
// 交易表单族读写接缝（#2034）：表单保单选项面（未删除保单）与基础收支写路径
// （创建 / 修改）直调域入口——与旧壳 create_transaction / update_transaction
// 及保单 store 消费的同一域函数，不经 IPC。商户 / 账户 / 分类选项面消费列表
// 装载的既有参考数据行集（boot::merchant_rows 等全量投影，在用口径在表单源
// 处过滤），不另立第二份读接缝。写成功后由视图广播 [`crate::signals`] 交易
// 失效信号（列表重读；即建商户证据经列表重读的商户行集刷新自然覆盖，旧壳
// write_entry 的参考失效信号在壳内等价物）。
// ---------------------------------------------------------------------------

/// 创建交易（域公开写入口直调，旧壳 `create_transaction` 命令同一执行协议）：
/// 创建编排入口 [`ledger_transaction::create`]——守卫 / 折算 / 落库 / op 产出
/// 在域内单正文（自持事务，中途失败整体回滚），余额重算由已接线的余额刷新
/// 接缝在既有事务内执行。写经壳层统一写入口直锁形态（提交点置脏，#2087）。
/// 返回写入证据（id + 是否即建商户）。写连接是成对建连的写侧。
pub fn create_transaction(
    db: &DbState,
    input: ledger_transaction::TransactionInput,
) -> Result<ledger_transaction::TransactionWrite> {
    write_entry_locked(db, |conn| ledger_transaction::create(conn, input))
}

/// 按 id 全字段替换交易（域公开写入口直调，旧壳 `update_transaction` 命令
/// 同一执行协议；与 HTTP `PUT /api/v1/transactions/{id}` 同一行为层权威）：
/// 修改编排入口 [`ledger_transaction::update`]。返回「是否即建商户」证据。
/// 不存在的 id 报码化 NotFound。写经壳层统一写入口直锁形态（#2087）。
pub fn update_transaction(
    db: &DbState,
    id: &str,
    input: ledger_transaction::TransactionInput,
) -> Result<ledger_infra::signals::WriteEvidence> {
    write_entry_locked(db, |conn| ledger_transaction::update(conn, id, input))
}

/// 读取未删除保单行集（表单保单下拉的数据源面，#2034）：旧壳保单 store 消费
/// `list_policies` 口径（软删不可再被新选择）。读连接是成对建连的只读侧。
pub fn policy_rows(db: &DbState) -> Result<Vec<ledger_policy::Policy>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_policy::list_policies(&conn)
}

/// 读取账本库数据期间边界（issue #2021 / ADR-0141 期间步进的钳制输入）：
/// 报表域日期极值读路径 [`ledger_reports::query_report_date_range`]——旧壳
/// report_date_range 消费的同一域函数；双 None = 空库。随每页读一并刷新
/// （单行聚合查询，成本可忽略；写路径落地后天然跟随数据外扩/收窄，
/// 无需独立失效接线）。
pub fn report_date_range(db: &DbState) -> Result<ledger_reports::DateRange> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_reports::query_report_date_range(&conn)
}

/// 读取账本库币种字典（金额展示的参考数据面，issue #2018）：装配层的币种
/// 展示参数（符号/小数位）来自本只读投影；随每页读一并刷新（字典行数小，
/// 改名/改号后的重读成本可忽略）。
pub fn currency_rows(db: &DbState) -> Result<Vec<Currency>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_currencies::list_currencies(&conn)
}

/// 读取在用分类行集（分类路径解析的参考数据面，issue #2019）：装配层的分类
/// 路径表来自本只读投影，**仅未删除**——旧壳路径函数消费在用分类行集
/// （reference store 口径），软删分类的历史交易引用在交易列表显示 `-`；随每页
/// 读一并刷新（分类行数小，改名/改父后的重读成本可忽略）。
pub fn category_rows(db: &DbState) -> Result<Vec<Category>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_categories::list_categories(&conn, false)
}

/// 读取用户可见账户行集（账户格与借贷派生的参考数据面，#2020）：装配层的账户
/// 表来自本只读投影，**在用且未隐藏**——旧壳 accountMap 消费 `list_accounts`
/// 口径（排除软删与隐藏；黑洞/隐藏账户不在表中 → 账户格占位「-」、借贷派生
/// 按缺失归一资金侧）；随每页读一并刷新（账户行数小，改名/改类型后的重读成本
/// 可忽略）。
pub fn account_rows(db: &DbState) -> Result<Vec<ledger_accounts::Account>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_accounts::list_accounts(&conn)
}

/// 读取商户行集（商户格的参考数据面，#2020）：装配层的商户表来自本只读投影，
/// **含软删全量**——旧壳 merchantMap 消费 `includeDeleted: true` 口径（在用 +
/// 软删，历史交易显示共用，issue #191）；随每页读一并刷新（商户行数小，改名后
/// 的重读成本可忽略）。
pub fn merchant_rows(db: &DbState) -> Result<Vec<ledger_merchants::Merchant>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_merchants::list_merchants(&conn, true)
}

// ---------------------------------------------------------------------------
// 预算读写接缝（#2033）：预算视图的读路径（进度 / 年度汇总 / 支出分类字典）
// 与写路径（创建 / 编辑 / 删除）直调 `ledger-budget` 域入口——与旧壳 budget
// 族命令消费同一域函数，不经 IPC。旧壳写入口的「预算写入刻意零信号」在壳内
// 对应物 = 本壳预算写成功后由视图广播 [`crate::signals`] 预算失效信号。
// ---------------------------------------------------------------------------

/// 「今天」（本地时区日历日）：预算进度窗口与年度汇总的同一时间储，与旧壳
/// `budget_progress` / `budget_annual_summary` 命令的 `Local::now().date_naive()`
/// 同口径（窗口随自然时间自动滚动）。
pub fn today_string() -> String {
    chrono::Local::now().date_naive().to_string()
}

/// 读取预算进度行集（旧壳 `budget_progress` 命令消费的同一域函数）：月预算
/// 统计当前自然月净支出（expense_net），年预算统计当前自然年累计；子分类支出
/// 并入父分类预算，子分类预算只算自身。读连接是成对建连的只读侧（ADR-0117）。
pub fn budget_progress(db: &DbState) -> Result<Vec<ledger_budget::BudgetProgress>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_budget::budget_progress_rows(&conn, chrono::Local::now().date_naive())
}

/// 读取预算年度汇总（旧壳 `budget_annual_summary` 命令消费的同一域函数）：
/// 年度预算总额（月预算年化）+ 本年已支 + 超标判定，默认币种单币种口径。
/// 读连接是成对建连的只读侧（ADR-0117）。
pub fn budget_annual_summary(db: &DbState) -> Result<ledger_budget::BudgetAnnualSummary> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_budget::budget_annual_summary(&conn, chrono::Local::now().date_naive())
}

/// 创建预算（域公开写入口直调，旧壳 `create_budget` 命令同一执行协议）：金额
/// / 支出分类 / 「分类 + 周期」唯一校验在域内，码化错误原样上抛（呈现面归
/// 视图层）。写连接是成对建连的写侧；op 随写事务提交/回滚（域编排内单点）。
pub fn create_budget(db: &DbState, input: &ledger_budget::BudgetInput) -> Result<String> {
    write_entry_locked(db, |conn| ledger_budget::crud::create_budget(conn, input))
}

/// 编辑预算金额与周期（域公开写入口直调，旧壳 `update_budget` 命令同一执行
/// 协议；分类不可改，改法为删旧建新）。写连接是成对建连的写侧。
pub fn update_budget(
    db: &DbState,
    id: &str,
    amount_cents: i64,
    period: Option<ledger_budget::BudgetPeriod>,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_budget::crud::update_budget(conn, id, amount_cents, period)
    })
}

/// 删除预算（软删；域公开写入口直调，旧壳 `delete_budget` 命令同一执行协议）。
/// 不存在的 id 报码化 NotFound（并发已删的呈现面归视图层）。写连接是成对建连
/// 的写侧。
pub fn delete_budget(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_budget::crud::delete_budget(conn, id))
}

// ---------------------------------------------------------------------------
// 概览视图读接缝（#2039）：四卡取数直调域入口——与旧壳 dashboard_overview /
// monthly_summary / list_holdings / cumulative_pnl_native_total /
// financial_freedom 命令消费的同一域函数，不经 IPC。各卡各一个读闭包：
// 折算与多语句同屏口径的同一读事务收口在域内单点（dashboard 读探针与
// financial_freedom 域内 ensure_transaction，tx_scope 快照纪律），本壳不另立
// 第二份口径表达式。
// ---------------------------------------------------------------------------

/// 净资产总览（dashboard 域读探针直调，旧壳 `dashboard_overview` 命令同一
/// 执行协议）：三腿合计（账户余额含投资现金 / 持仓市值 / 实物资产）与多币
/// 种折算全在后端单点（ADR-0131 同思路：总览屏不出现第二份口径表达式）。
/// **走写连接**（旧壳同款只读甄别收口，ADR-0117 代价 3）：域读探针内含缓存
/// 自愈写（指纹失配实时重算并 UPSERT 净资产缓存），只读读连接会在只读约束
/// 上报错。
pub fn dashboard_overview(db: &DbState) -> Result<ledger_dashboard::DashboardOverview> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_dashboard::query_dashboard_overview(&conn)
}

/// 月度收支行集（报表域读路径直调，旧壳 `monthly_summary` 命令同一执行协议）：
/// 指定年份逐月行（income_net / 毛支出 / 退款），当月行由视图按本月键拾取。
/// 读连接是成对建连的只读侧（ADR-0117）。
pub fn monthly_summary_rows(
    db: &DbState,
    year: i64,
) -> Result<Vec<ledger_reports::MonthlySummary>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_reports::monthly_summary_rows(&conn, year, None, None)
}

// ---------------------------------------------------------------------------
// 报表视图读接缝（#2048）：期间口径的三张报表直调报表域入口——与旧壳
// monthly_summary / category_shares / merchant_shares 命令的 from/to 期间
// 形态消费的同一域函数，不经 IPC。期间快照 = 报表页会话态（ADR-0057 四枚
// 日期芯片，双端必有界）。读连接是成对建连的只读侧（ADR-0117）。
// ---------------------------------------------------------------------------

/// 月度汇总（期间内按月分布口径，issue #411 定型）：期间快照进，域聚合
/// 分组按月不变、无流水的月份不成行。遗留 `year` 参数不参与（期间口径下）。
pub fn monthly_summary_for_period(
    db: &DbState,
    range: &crate::period::PeriodRange,
) -> Result<Vec<ledger_reports::MonthlySummary>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_reports::monthly_summary_rows(&conn, 0, Some(&range.from), Some(&range.to))
}

/// 分类份额（净额，issue #376 母集 + #411 期间化）：支出聚合 `expense_net`
/// 口径，期间快照进；缺省月份/年份遗留参数全部不参与（期间口径）。
pub fn category_shares_rows(
    db: &DbState,
    range: &crate::period::PeriodRange,
) -> Result<Vec<ledger_reports::CategoryShare>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_reports::category_shares_rows(
        &conn,
        "expense",
        None,
        None,
        Some(&range.from),
        Some(&range.to),
    )
}

/// 商户消费排行（净额，issue #192 + #588 topN 截断 + #411 期间化）：期间快照
/// 进，载荷 = 排行行 + 全量合计（占比分母，与截断无关）。
pub fn merchant_shares_report(
    db: &DbState,
    range: &crate::period::PeriodRange,
    top_n: Option<i64>,
) -> Result<ledger_reports::MerchantSharesReport> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_reports::merchant_shares_report(&conn, 0, Some(&range.from), Some(&range.to), top_n)
}

/// 当前持仓行集（投资域读路径直调，旧壳 `list_holdings` 命令同一执行协议）：
/// `v_holdings` 行 + 账户币逐行当期汇率软折算（`native_*_cents` 行级空值
/// 语义——缺现价或缺折算汇率行不计入合计，呈现面显式可见）。投资概览卡与
/// 财务自由度分子（持仓市值腿）同源不漂移。读连接是成对建连的只读侧。
pub fn portfolio_rows(db: &DbState) -> Result<Vec<ledger_investment::Holding>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::list_holdings(&conn)
}

/// 累计收益·折本位币单值（投资域读路径直调，旧壳 `cumulative_pnl_native_total`
/// 命令同一执行协议）：三腿逐行当期汇率折全局默认币种后求和（缺汇率码化
/// 上抛，域内同事务收口）。读连接是成对建连的只读侧（ADR-0117）。
pub fn cumulative_pnl_native_total(
    db: &DbState,
) -> Result<ledger_investment::CumulativePnlNativeTotal> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::query_cumulative_pnl_native_total(&conn)
}

/// 财务自由度总览（投资域读路径直调，旧壳 `financial_freedom` 命令同一执行
/// 协议）：分子（可投资资产两腿折算）与分母（预算域年化）跨表多语句，域内
/// `ensure_transaction` 同一读事务（分子分母恒同时点，tx_scope 快照纪律）。
/// 读连接是成对建连的只读侧（ADR-0117）。
pub fn financial_freedom(db: &DbState) -> Result<ledger_investment::FinancialFreedomOverview> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::query_financial_freedom(&conn)
}

/// 投资概览读数（投资页「概览」页签读接缝，旧壳 `investment_overview` 命令
/// 消费的同一域函数，不经 IPC；ADR-0131 全页折本位币单值，装配层只消费不另
/// 立口径）：可投资资产两腿 + 投资合计三项 + 缺价计数与引导判定多段取数，
/// 域内 `ensure_transaction` 同一读事务（#1699 同快照纪律）。读连接是成对
/// 建连的只读侧（ADR-0117）。
pub fn investment_overview(db: &DbState) -> Result<ledger_investment::InvestmentOverview> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::query_investment_overview(&conn)
}

/// 已实现盈亏汇总（投资页「盈亏」页签读接缝，旧壳 `realized_pnl_summary` 命
/// 令消费的同一域函数，不经 IPC；账户/标的筛选与两腿折算、`fx.rate-missing`
/// 码化上抛全在域内）：按年与按账户两张单值表多语句 + 汇率装载，域内
/// `ensure_transaction` 同一读事务（#1699 同快照纪律）。读连接是成对建连的
/// 只读侧（ADR-0117）。
pub fn realized_pnl_summary(
    db: &DbState,
    filter: &ledger_investment::PnlFilter,
) -> Result<ledger_investment::RealizedPnlSummary> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::query_realized_pnl_summary(&conn, filter)
}

/// 组合市值走势（走势卡读接缝，旧壳 `portfolio_value_trend` 命令消费的同一
/// 域函数，不经 IPC；区间裁剪与周采样全在域内，逐周数量×周线价×同期汇率
/// 多语句读闭包在域内 `ensure_transaction` 收同一读事务）。读连接是成对建
/// 连的只读侧（ADR-0117）。
pub fn portfolio_value_trend(
    db: &DbState,
    range: &ledger_investment::TrendRange,
) -> Result<ledger_investment::PortfolioValueTrend> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::query_portfolio_value_trend(&conn, range)
}

/// 资金加权收益率汇总（盈亏页收益率卡读接缝，旧壳
/// `money_weighted_return_summary` 命令消费的同一域函数，不经 IPC；现金流集
/// 与边界市值多语句读闭包在域内 `ensure_transaction` 收同一读事务）。旧壳
/// 调用不传区间（缺省 = 自首笔流水起算、截至今日现值），本壳同口。读连接
/// 是成对建连的只读侧（ADR-0117）。
pub fn money_weighted_return_summary(
    db: &DbState,
) -> Result<ledger_investment::MoneyWeightedReturnSummary> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::query_money_weighted_return_summary(
        &conn,
        &ledger_investment::MwrRange::default(),
    )
}

/// 标的全集（盈亏页标的筛选候选面）：旧壳盈亏筛选是远程搜索
/// `list_instruments`（每输入一发命令），本壳一次分页循环装全集后经统一模糊
/// 搜索语义本地过滤（[`crate::pinyin_select::filter_options`] 消费的同一域
/// 函数 `list_instruments`，search 语义同源）——筛选语义平价、无远程竞态；
/// 全集装载上限随 `page_size` 最大化并以 `total` 收核（半途即止会静默截断
/// 候选面，不做）。读连接是成对建连的只读侧（ADR-0117）。
pub fn instrument_rows(db: &DbState) -> Result<Vec<ledger_investment::Instrument>> {
    let conn = conn_lock(&db.read_conn)?;
    let page_size = ledger_investment::list_instruments(&conn, &Default::default())?
        .total
        .clamp(1, 500) as usize;
    let mut all = Vec::new();
    let mut page = 1usize;
    loop {
        let result = ledger_investment::list_instruments(
            &conn,
            &ledger_investment::InstrumentListFilter {
                page: Some(page),
                page_size: Some(page_size),
                ..Default::default()
            },
        )?;
        let fetched = result.items.len();
        all.extend(result.items);
        if all.len() >= result.total.max(0) as usize || fetched == 0 {
            break;
        }
        page += 1;
    }
    Ok(all)
}

/// 价格过期检查（#2046 持仓页签同步接缝的背景提示面；旧壳
/// `instrument_price_staleness` 命令消费的同一域函数，不经 IPC）：本地
/// 水位检查口径单点在域（计数与阈值都归域），本壳只搬运计数（旧壳
/// usePriceStaleness 同纪律）。读连接是成对建连的只读侧（ADR-0117）。
pub fn instrument_price_staleness(db: &DbState) -> Result<ledger_investment::PriceStaleness> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::instrument_price_staleness(&conn)
}

/// 读取一笔 buy/sell 交易的买卖明细（投资记账表单编辑回填，#2046；旧壳
/// `get_transaction_trade` 命令消费的同一域函数，不经 IPC）：标的/数量/价格/
/// 费用 + 标的字典带出字段（标的类型决定录入权威分流）。无明细（非 buy/sell）
/// 码化 NotFound。读连接是成对建连的只读侧（ADR-0117）。
pub fn transaction_trade(
    db: &DbState,
    transaction_id: &str,
) -> Result<ledger_investment::TransactionTrade> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::get_transaction_trade(&conn, transaction_id)
}

/// 投资明细列表页（投资页「明细」页签读接缝，旧壳 `list_investment_transactions`
/// 命令消费的同一域函数，不经 IPC）：五种投资 kind 行 + 四维过滤 + 服务端
/// offset 分页（items 与 total 同快照——读闭包收进域内读事务）。本页签页
/// 大小单点在壳（[`LEDGER_TAB_PAGE_SIZE`]），过滤维与页码由视图装配直传。
/// 读连接是成对建连的只读侧（ADR-0117）。
pub fn investment_transaction_page(
    db: &DbState,
    filter: &ledger_investment::InvestmentTransactionListFilter,
) -> Result<ledger_investment::InvestmentTransactionListResult> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::list_investment_transactions(&conn, filter)
}

/// 明细页签页大小（旧壳 LEDGER_TAB_PAGE_SIZE_DEFAULT 同值同源：固定 20）。
pub(crate) const LEDGER_TAB_PAGE_SIZE: usize = 20;

/// 标的列表页（投资页「标的」页签读接缝，旧壳 `list_instruments` 命令消费
/// 的同一域函数，不经 IPC）：搜索（统一模糊语义——锚点定位、COUNT 与 items
/// 读闭包在域内收同一读事务）+ 市场精确匹配 + 只看持仓 + anchor 分页定位。
/// 读连接是成对建连的只读侧（ADR-0117）。
pub fn instrument_page(
    db: &DbState,
    filter: &ledger_investment::InstrumentListFilter,
) -> Result<ledger_investment::InstrumentListResult> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::list_instruments(&conn, filter)
}

/// 手动报价（投资页「标的」页签行内「录价」写入的域直调面；旧壳
/// `record_manual_price` 命令写路径的本壳等价半边，不经 IPC）：一条通道两个
/// 落点（现价缓存 upsert + 价格历史周采样幂等覆盖——回填旧价只沉淀历史，
/// 最新点映像规则在域）。写入经统一写入口直锁形态（issue #2087）：实际写入
/// 的提交点置脏由连接层单点结构性生效（整段形态，无需旧壳分段收尾的空写作
/// 业）；「是否广播」按写入见证（任一落点实际写入）由壳侧裁决（信号映射判
/// 定单点在 infra signals；本壳投影到投资族信号通道）。写连接是成对建连的写侧。
pub fn record_manual_price(
    db: &DbState,
    input: &ledger_investment::ManualPriceInput,
) -> Result<ledger_investment::ManualPriceResult> {
    write_entry_locked(db, |conn| {
        ledger_investment::record_manual_price(conn, input)
    })
}

/// 自建标的创建（旧壳 `create_instrument` 命令写路径的本壳等价半边，不经
/// IPC）：手动创建入口守卫（类型白名单 + 名称必填，ADR-0036 决策 3）在域
/// 写路径单正文，本地写编排（建档/复用改名 + op 追加）同收一事务。（代码，
/// 类型）幂等：同键复用并只更新名称。创建恒零价格证据（不落现价）——
/// 广播判定归调用方签名直接按零证据裁决（不广播，旧壳 Silent 写入口同判）。
pub fn create_instrument(
    db: &DbState,
    input: ledger_investment::InstrumentInput,
) -> Result<String> {
    write_entry_locked(db, |conn| {
        ledger_investment::create_instrument_manual(conn, input)
    })
}

/// 自建标的删除（旧壳 `delete_instrument` 命令写路径的本壳等价半边，不经
/// IPC）：仅手动来源且无 buy/sell 流水引用（守卫在域写路径），同步来源与
/// 被引用标的拒删（级联：现价缓存/价格历史 CASCADE、批次被守卫覆盖）。
/// 删除是字典清理不是价格写入——零价格证据（不广播，旧壳 DeleteInstrument
/// 信号映射 NO_SIGNALS 同判）。
pub fn delete_instrument(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_investment::delete_instrument(conn, id))
}

/// 添加投资标的·场外基金通道（旧壳 `add_fund_by_code` 命令壳的本壳等价半
/// 边，不经 IPC）：格式校验即刻拒绝（不发网络请求）→ 行情拉取在连接锁外
/// await（单请求叠加限流冷却重试最长可达分钟级，任何形状下不进锁）→ 落库
/// 经域公开写入口（拉取已完成，注入闭包同步回放结果——与旧壳/BDD 同一套
/// 校验→拉取→落库实现）。落现价的证据随结果返回，广播判定归调用方签名。
pub async fn add_fund_by_code(
    db: &DbState,
    code: &str,
) -> Result<ledger_investment::AddFundResult> {
    ledger_investment::validate_fund_code(code)?;
    let quote = ledger_market_sync::fetch_fund_quote_production(code).await?;
    write_entry_locked(db, |conn| {
        let mut fetch = |_: &str, _: &str| Ok(quote.clone());
        ledger_investment::add_fund_by_code_with(conn, code, &mut fetch)
    })
}

/// 添加投资标的·场内通道（沪/深/港/美股；旧壳 `add_instrument_by_code` 命
/// 令壳的本壳等价半边，不经 IPC）：查询阶段在连接锁外 await（通道解析 →
/// 查询单元解析 → 腾讯行情）→ 识别落库经同一接缝（类型自动识别，行情命
/// 中 → stock、类型特征 → etf）。查询未命中与临时不可达以码化错误上抛。
pub async fn add_instrument_by_code(
    db: &DbState,
    market: &str,
    code: &str,
) -> Result<ledger_investment::AddStockInstrumentResult> {
    let mut fetch = |code: &str, route: ledger_investment::StockRoute| {
        let code = code.to_string();
        async move { ledger_market_sync::fetch_stock_quote_production(route, &code).await }
    };
    let quote = ledger_investment::fetch_stock_quote_for_add(market, code, &mut fetch).await?;
    write_entry_locked(db, |conn| {
        ledger_investment::add_stock_instrument_with_quote(conn, &quote)
    })
}

/// 持仓标的同步编排接缝（#2046；旧壳 `sync_instrument_info` IPC 命令壳的
/// 本壳等价形，消费面在 [`crate::investment_sync`] 同步接缝，不经 IPC）：
/// 门面写槽裸作业会话交给投资同步编排（旧壳命令消费的同一域编排入口
/// [`ledger_market_sync::do_incremental_sync_channels`]），进度回调由壳侧
/// 注入（消费端把进度报到接缝信号的呈现位）。
///
/// 收尾裁决（旧壳 write_entry_segmented_async 的本壳等价半边）：跨分段写入
/// 见证随结果必达（成败同判——失败 ≠ 未写过，中途失败已落库的写入仍生效，
/// issue #1277）；分段裸作业不经写入口，旧壳「实际写过即置脏」的空写作业
/// 收尾本壳不重演（分段面置脏接通属设计裁决，偏差留痕见 #2087 交付报告）
/// ——「是否广播」直接按见证由壳侧裁决（信号映射判定单点在 infra signals：
/// SyncInstrumentInfo × 实际写过 → PRICES_CHANGED，本壳投影到投资族信号通
/// 道）。进度不是失效信号（issue #897），只进呈现位。
pub async fn sync_instrument_info(
    db: &DbState,
    mut progress: impl FnMut(ledger_market_sync::SyncProgress) + Send,
) -> SyncInstrumentInfoOutcome {
    let session =
        ledger_market_sync::FacadeWriteSession::new(db.write_handle(), "sync_instrument_info");
    // 通道束（旧壳 #1276 换装口径）：生产默认生产通道束（每次同步构造，与旧
    // 壳整段形态同口径；NSH 无测试注入口——双壳并存期同步联调走旧壳）。
    match ledger_market_sync::SyncFetchChannels::production() {
        Ok(mut channels) => {
            let mut witness = ledger_market_sync::WriteWitness::default();
            let outcome = ledger_market_sync::do_incremental_sync_channels(
                &session,
                &mut channels,
                &mut progress,
                &mut witness,
            )
            .await;
            let evidence = witness.any_written();
            match outcome {
                Ok(result) => SyncInstrumentInfoOutcome::Success(result, evidence),
                Err(error) => SyncInstrumentInfoOutcome::Failed(error, evidence),
            }
        }
        Err(error) => SyncInstrumentInfoOutcome::Failed(error, false),
    }
}

/// 同步一次的终态归一（旧壳命令收尾裁决的投影面）：成功摘要 / 失败错误
/// 二选一 + 跨分段累积的写入证据（成败同判的最小幸存事实）。
#[derive(Debug)]
pub enum SyncInstrumentInfoOutcome {
    /// 编排成功终态：结果统计（同步 N 只 / 跳过 M 只 / 结果文案）+ 是否实际
    /// 写过（信号广播消费面）。
    Success(ledger_market_sync::SyncInstrumentInfoResult, bool),
    /// 编排失败终态：错误原样（消费方呈现失败文案——旧壳失败错误在发射后
    /// 原样上抛的同位）+ 是否实际写过（失败路径证据必达，issue #1277）。
    Failed(ledger_infra::error::AppError, bool),
}

// ---------------------------------------------------------------------------
// 账户视图读写接缝（#2041）：账户余额清单（V017 持久化缓存口径）与目标绑定
// 集 / 计划引用集直调域入口——与旧壳 list_account_balances、
// savingsGoalProgress、listScheduledTransactions 命令消费的同一域函数，不经
// IPC。账户写路径（创建 / 编辑 / 删除 / 归档 / 恢复 / 余额调整）直调域公开
// 写入口——与旧壳账户族命令同一执行协议；写连接是成对建连的写侧。
// ---------------------------------------------------------------------------

/// 账户余额行集（旧壳 `list_account_balances` 命令同一执行协议）：**不含黑洞
/// 账户**（include_hidden = false，用户侧查询面板径）。余额读 V017 持久化
/// 缓存（ADR-0067）：行读数来自域余额缓存行（写路径在写事务内对受影响账户
/// 整体重算回写），本壳只消费不另立算术；缓存行缺失报码化错误
/// （`balance.cache-row-missing`），不静默回退。读连接是成对建连的只读侧
/// （ADR-0117）。
pub fn account_balance_rows(db: &DbState) -> Result<Vec<ledger_accounts::AccountBalance>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_accounts::list_account_balances_with_visibility(&conn, false)
}

/// 储蓄目标进度行集（旧壳 savingsGoalProgress 命令消费的同一域函数）：账户页
/// 「储蓄目标」分组与「名称随动只读」判定的绑定集消费面——未删除目标全集
/// （含已归档目标，旧壳 store goalAccountIds 同口径）。读连接是成对建连的
/// 只读侧；读失败由视图呈现加载错误（不用绑集判定照常回退为「无绑定」会静默
/// 改分组，不做降级）。
pub fn savings_goal_progress_rows(
    db: &DbState,
) -> Result<Vec<ledger_savings_goal::SavingsGoalProgress>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_savings_goal::list_savings_goal_progress(&conn, chrono::Local::now().date_naive())
}

/// 定时计划行集（旧壳 `listScheduledTransactions` 命令消费的同一域函数，软删
///滤除在域内）：归档确认框「未结束计划数」提示的引用集消费面（提示是佐证
/// 信息，读失败按 0 处理的丢弃语义归视图侧，与旧壳 plan-usage 消费同口径）。
/// 读连接是成对建连的只读侧。
pub fn plan_rows(db: &DbState) -> Result<Vec<ledger_scheduled::ScheduledTransactionWithExt>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_scheduled::list_plans(&conn)
}

/// 创建账户（域公开写入口直调，旧壳 `create_account` 命令同一执行协议）：
/// 编排入口 [`ledger_accounts::create_account`]——落库 + 缓存行 + op 产出在域
/// 内单正文（自持事务，中途失败整体回滚）。写连接是成对建连的写侧。
pub fn create_account(db: &DbState, input: ledger_accounts::AccountInput) -> Result<String> {
    write_entry_locked(db, |conn| ledger_accounts::create_account(conn, input))
}

/// 按 id 全字段替换账户编辑面（域公开写入口直调，旧壳 `update_account` 命令
/// 同一执行协议）：归档可选键携带 `true` 时先经目标域守卫（旧壳命令壳同位，
/// 见 [delete_account] 同形理由）。不存在的 id 报码化 NotFound。写连接是成对
/// 建连的写侧。
pub fn update_account(
    db: &DbState,
    id: &str,
    input: ledger_accounts::AccountUpdateInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        if input.is_archived == Some(true) {
            ledger_savings_goal::ensure_account_can_archive(conn, id)?;
        }
        ledger_accounts::update_account(conn, id, input)
    })
}

/// 删除账户（软删；域公开写入口直调，旧壳 `delete_account` 命令同一执行协议）：
/// 目标域守卫在入口先行（「删账户命令先经目标域校验」，旧壳命令壳同位——账户
/// 域不反向依赖目标域，ADR-0133 决策 4）：在用目标的专属账户被码化拒绝，普通
/// 账户照常放行。写连接是成对建连的写侧。
pub fn delete_account(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::ensure_account_not_goal_bound(conn, id)?;
        ledger_accounts::delete_account(conn, id)
    })
}

/// 归档账户（幂等；域入口直调，旧壳 `archive_account` 命令同一执行协议）：
/// 绑定目标的账户被码化拒绝（守卫住目标域，旧壳命令壳同位）。写连接是成对
/// 建连的写侧。
pub fn archive_account(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::ensure_account_can_archive(conn, id)?;
        ledger_accounts::set_account_archived(conn, id, true)
    })
}

/// 恢复账户（归档的唯一回退动作；域入口直调，旧壳 `unarchive_account` 命令
/// 同一执行协议）。写连接是成对建连的写侧。
pub fn unarchive_account(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_accounts::set_account_archived(conn, id, false)
    })
}

/// 余额调整（域公开写入口直调，旧壳 `adjust_account_balance` 命令同一执行
/// 协议）：把余额校准到目标值，机制为生成一笔与黑洞账户的转账（ADR-0026）；
/// 黑洞账户 ensure 与交易写入同事务（域内 [`ledger_accounts::adjust_account_balance`]
/// 的外层事务壳承担）。返回（新交易 id, 是否按需新建了黑洞账户）。写连接是
/// 成对建连的写侧。
pub fn adjust_account_balance(
    db: &DbState,
    id: &str,
    input: &ledger_accounts::AccountBalanceAdjustInput,
) -> Result<(String, bool)> {
    write_entry_locked(db, |conn| {
        ledger_accounts::adjust_account_balance(conn, id, input)
    })
}

// ---------------------------------------------------------------------------
// 定时计划读写接缝（#2042）：订阅式计划清单 / 计划详情 / 订阅花费总览读 +
// 计划写入（建档 / 状态 / 编辑 / 期次执行 / 期次展开）+ 即建商户直调
// `ledger-scheduled` / `ledger-merchants` 域入口——与旧壳 scheduled 族命令
//（listScheduledTransactions / getScheduledTransactionDetail /
// subscriptionSpendOverview / createScheduledTransaction /
// updateScheduledTransactionStatus / updateScheduledSubscription /
// executeScheduledOccurrence / expandScheduledOccurrences / createMerchant）
// 消费同一域函数，不经 IPC。写连接是成对建连的写侧，读连接是只读侧。
// 旧壳「计划写入刻意零信号」在壳内对应物 = 本壳视图层在写成功回调里自行
// 重拉（清单 / 花费面板）；期次执行落交易行属交易域变化，视图层手动广播
// [`crate::signals`] 交易失效信号（交易列表重读）。
// ---------------------------------------------------------------------------

/// 计划完整详情（旧壳 `getScheduledTransactionDetail` 命令消费的同一域函数）：
/// 含扩展字段与全量期次（含 failed / cancelled 各状态，按 scheduled_date
/// 升序）。计划级读快照一致性收口在域内 `ensure_transaction`。读连接是成对
/// 建连的只读侧（ADR-0117）。
pub fn scheduled_plan_detail(
    db: &DbState,
    id: &str,
) -> Result<ledger_scheduled::ScheduledTransactionDetail> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_scheduled::get_plan_detail(&conn, id)
}

/// 订阅花费总览（旧壳 `subscriptionSpendOverview` 命令消费的同一域函数）：
/// 逐订阅行 + 本月 / 本年实际花费 + 过去 12 个月逐月序列 + 折算月 / 年推算
/// 成本（本位币）。今日取本地时区日历日（旧壳命令层注入同口径）。读失败
///（如本位币缺汇率）不静默混算：面板呈现失败态（旧壳 loadFailed 同语义）。读
/// 连接是成对建连的只读侧（ADR-0117）。
pub fn subscription_spend_overview(
    db: &DbState,
) -> Result<ledger_scheduled::SubscriptionSpendOverview> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_scheduled::query_subscription_spend(&conn, chrono::Local::now().date_naive())
}

/// 创建定时交易计划（域公开写入口直调，旧壳 `createScheduledTransaction`
/// 命令同一执行协议）：建档协议（校验 + 核心表 / 扩展表 + 预生成期次）与 op
/// 产出一事务收口在域内 create_plan（自持事务）。返回计划 id。写连接是成对
/// 建连的写侧。
pub fn create_scheduled_plan(
    db: &DbState,
    input: ledger_scheduled::CreateScheduledInput,
) -> Result<String> {
    write_entry_locked(db, |conn| ledger_scheduled::create_plan(conn, input))
}

/// 变更计划状态（域公开写入口直调，旧壳 `updateScheduledTransactionStatus`
/// 命令同一执行协议）：守护裁决 + 取消级联在域内（opcode 单点）。写连接是
/// 成对建连的写侧。
pub fn update_scheduled_plan_status(
    db: &DbState,
    id: &str,
    new_status: ledger_scheduled::ScheduledStatus,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_scheduled::update_plan_status(conn, id, new_status)
    })
}

/// 编辑订阅计划的非金额字段（域公开写入口直调，旧壳
/// `updateScheduledSubscription` 命令同一执行协议）：金额哨兵拒绝、商户保持
/// 历史引用语义与「保单缴费协议不挂商户」裁决全在域内单点。写连接是成对
/// 建连的写侧。
pub fn update_scheduled_subscription(
    db: &DbState,
    input: ledger_scheduled::UpdateSubscriptionInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_scheduled::update_subscription(conn, input)
    })
}

/// 执行一条 pending / failed 期次（域公开写入口直调，旧壳
/// `executeScheduledOccurrence` 命令同一执行协议）：经 Writer 接缝归一化折算
/// 落交易行、回填期次、计划完成检查全在域内单正文（事务自持）。返回落地
/// 交易 id。写连接是成对建连的写侧。
pub fn execute_scheduled_occurrence(db: &DbState, occurrence_id: &str) -> Result<String> {
    write_entry_locked(db, |conn| {
        ledger_scheduled::execute_occurrence(conn, occurrence_id)
    })
}

/// 预生成下一批期次（域公开写入口直调，旧壳 `expandScheduledOccurrences`
/// 命令同一执行协议）：返回本次新生成的期次 id 列表。写连接是成对建连的
/// 写侧。
pub fn expand_scheduled_occurrences(db: &DbState, id: &str) -> Result<Vec<String>> {
    write_entry_locked(db, |conn| ledger_scheduled::expand_occurrences(conn, id))
}

/// 按名即建商户（域公开按名 find-or-create 直调，旧壳 `createMerchant`
/// 命令的重名车间同义——`create_merchant_by_name` 的精确匹配在用名复用 /
/// 未命中即建语义在域内单点，重名陈旧态竞态不再前端逐层兜底）。计划表单
///「输入即建」的自由文本提交路径消费面（旧壳 resolveMerchantRef 的 NSH
/// 等价路径 收口在装配单点）。写连接是成对建连的写侧。
pub fn create_merchant_by_name(db: &DbState, name: &str) -> Result<String> {
    write_entry_locked(db, |conn| {
        ledger_merchants::create_merchant_by_name(conn, name)
    })
}

// ---------------------------------------------------------------------------
// 商户字典管理读写接缝（#2052）：旧壳 MerchantManager 消费的 createMerchant /
// updateMerchant / deleteMerchant / listMerchantTransactionCounts 命令的同一域
// 函数直调，不经 IPC。商户域写语义零改动（Out of Scope：只迁 UI），唯一性
// 守卫 / 软删 / 历史引用保留全在域内单正文；读接缝复用 [`merchant_rows`]
// 全量投影不另立（清单在用 / 已删分区在装配层划分）。写成功后由视图广播
// [`crate::signals`] 交易失效信号（交易列表来源列展示商户名 / 表单商户源 /
// 筛选商户下拉——旧壳 ledger:changed 参考失效信号的壳内等价物；引用计数
// 读失败静默保留旧值是旧壳 listMerchantTransactionCounts 消费面同口径）。
// ---------------------------------------------------------------------------

/// 创建商户（域公开写入口直调，旧壳 `createMerchant` 命令同一执行协议）：
/// 在用重名被域守卫码化拒绝（`merchant.already-exists`），零落库。返回新商户
/// id。写连接是成对建连的写侧。
pub fn create_merchant(db: &DbState, input: ledger_merchants::MerchantInput) -> Result<String> {
    write_entry_locked(db, |conn| ledger_merchants::create_merchant(conn, input))
}

/// 改名商户（域公开写入口直调，旧壳 `updateMerchant` 命令同一执行协议）：
/// trim 归一 / 空名与在用撞名由域守卫码化拒绝，改名即时生效（交易以
/// merchant_id 引用，不回刷历史交易行——ADR-0028）。写连接是成对建连的写侧。
pub fn update_merchant(
    db: &DbState,
    id: &str,
    input: ledger_merchants::MerchantUpdateInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_merchants::update_merchant(conn, id, input)
    })
}

/// 软删商户（域公开写入口直调，旧壳 `deleteMerchant` 命令同一执行协议）：
/// 历史交易引用照常显示（软删商户不可再被新交易选择）；不存在的 id 报码化
/// NotFound。写连接是成对建连的写侧。
pub fn delete_merchant(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_merchants::delete_merchant(conn, id))
}

/// 读取商户关联交易计数（旧壳 `listMerchantTransactionCounts` 命令消费的同一
/// 域函数）：每商户被未删流水引用的毛笔数（含软删行、LEFT JOIN 全字典、缺失
/// 补 0 在域内），实时推导不落库。读连接是成对建连的只读侧。
pub fn merchant_transaction_counts(
    db: &DbState,
) -> Result<Vec<ledger_merchants::MerchantTransactionCount>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_merchants::transaction_counts(&conn)
}

// 储蓄目标视图读写接缝（#2056）：蓄水进度读直调域读路径（旧壳
// savingsGoalProgress 命令消费的同一域函数，#2041 账户视图绑集读同点消费），目标写路径
// （创建 / 编辑 / 归档 / 取消归档 / 删除）直调域公开写入口——与旧壳
// create/update/archive/unarchive/delete_savings_goal 命令同一执行协议，不经
// IPC。生命周期守卫（余额非零拒绝删除、归档不删数据）住域单正文，本壳不自
// 建守卫语义（spec #2056：生命周期守卫语义以域为唯一真源）。删除级联软删
// 专属账户随域同一事务。写连接是成对建连的写侧。
// ---------------------------------------------------------------------------

/// 创建储蓄目标（域公开写入口直调，旧壳 create_savings_goal 命令同一执行
/// 协议）：目标金额正数守卫在域内码化拒绝；同一事务经账户域公开写入口自动
/// 建专属账户（1:1 绑定）。返回目标 id。写连接是成对建连的写侧。
pub fn create_savings_goal(
    db: &DbState,
    input: &ledger_savings_goal::SavingsGoalInput,
) -> Result<String> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::create_savings_goal(conn, input)
    })
}

/// 编辑储蓄目标（四字段全量替换，域公开写入口直调，旧壳 update_savings_goal
/// 命令同一执行协议）：名称变化联动专属账户名（目标名权威），非法输入
///（目标额非正 / 空名 / 计划月存非正）由域守卫码化拒绝零落库。写连接是成对
/// 建连的写侧。
pub fn update_savings_goal(
    db: &DbState,
    id: &str,
    input: &ledger_savings_goal::SavingsGoalUpdateInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::update_savings_goal(conn, id, input)
    })
}

/// 归档储蓄目标（域入口直调，旧壳 archive_savings_goal 命令同一执行协议）：
/// 退出默认列表，账户 / 流水 / 关联计划原样保留。写连接是成对建连的写侧。
pub fn archive_savings_goal(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::archive_savings_goal(conn, id)
    })
}

/// 取消归档储蓄目标（域入口直调，旧壳 unarchive_savings_goal 命令同一执行
/// 协议）：恢复 active 回默认列表。写连接是成对建连的写侧。
pub fn unarchive_savings_goal(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::unarchive_savings_goal(conn, id)
    })
}

/// 删除储蓄目标（域公开写入口直调，旧壳 delete_savings_goal 命令同一执行
/// 协议）：余额非零被域守卫码化拒绝（引导先转出），余额为零时域内同一事务
/// 目标软删 + 级联软删专属账户（历史流水保留可查）。写连接是成对建连的写侧。
pub fn delete_savings_goal(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::delete_savings_goal(conn, id)
    })
}

// 保单视图读写接缝（#2050）：保单清单 / 保单视角统计 / 保司字典（含软删显示
// 映射面）直调 `ledger-policy` 域入口——与旧壳 list_policies / list_policy_stats
// / create / update / delete_policy 与保司（resolve-insurer 的 create_insurer
// 消费面）命令消费的同一域函数，不经 IPC。保单写路径的校验（保司在用、保单号
// / 险种非空、期间、保额币种成对）与失效信号回调注入全在域内，本壳不自建语义。
// 写连接是成对建连的写侧，读连接是只读侧（ADR-0117）。
// ---------------------------------------------------------------------------

/// 保单视角统计行集（旧壳 `list_policy_stats` 命令消费的同一域函数，逐保单
/// 合计 + 下期扣款日 + 到期态实时推导不落库，issue #363）。多语句读闭包的
/// 同事务收口在域内 `ensure_transaction`（#1702 tx_scope 纪律），本壳只消费
/// 不另立口径。`today` 取本地日历日与旧壳命令层注入同口径。读连接是成对建
/// 连的只读侧。
pub fn policy_stats_rows(db: &DbState) -> Result<Vec<ledger_policy::PolicyStats>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_policy::policy_stats(&conn, chrono::Local::now().date_naive())
}

/// 创建保单（域公开写入口直调，旧壳 `create_policy` 命令同一执行协议）：校验
/// 与归一化（保单号 / 险种非空、期间可解析且止日不早于起日、保额币种成对）
/// 在域内，码化错误原样上抛（呈现面归视图层）。写连接是成对建连的写侧。
pub fn create_policy(db: &DbState, input: ledger_policy::PolicyInput) -> Result<String> {
    write_entry_locked(db, |conn| {
        ledger_policy::create_policy(conn, input, &mut || {})
    })
}

/// 按 id 编辑保单静态要素（域公开写入口直调，旧壳 `update_policy` 命令同一
/// 执行协议）：全量替换；保司「保持历史引用」语义（携带保司与既有相同 = 跳过
/// 在用校验，软删保司的历史保单仍可编辑其他要素）在域内单点。写连接是成对
/// 建连的写侧。
pub fn update_policy(db: &DbState, id: &str, input: ledger_policy::PolicyInput) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_policy::update_policy(conn, id, input, &mut || {})
    })
}

/// 软删除保单（域公开写入口直调，旧壳 `delete_policy` 命令同一执行协议）：
/// `is_deleted=1` 不物理移除，库内行与历史引用保留不置空（ADR-0051 决策 5）。
/// 写连接是成对建连的写侧。
pub fn delete_policy(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_policy::delete_policy(conn, id, &mut || {})
    })
}

/// 保司按名 find-or-create（域公开直调，旧壳 `createInsurer` 命令的重名兜底
/// 同义——`create_insurer_by_name` 的「输入名精确命中在用保司复用 / 未命中
/// 即建」语义在域内单点，重名陈旧态竞态不再前端逐层兜底；本壳保存时数据
/// 现读，无 store 缓存竞态面）。保单表单「输入即建」路径消费面。写连接是
/// 成对建连的写侧。
pub fn create_insurer_by_name(db: &DbState, name: &str) -> Result<String> {
    write_entry_locked(db, |conn| ledger_policy::create_insurer_by_name(conn, name))
}

// ---------------------------------------------------------------------------
// 保司管理读写接缝（#2051）：保司字典清单读与写路径（创建 / 改名 / 软删）
// 直调 `ledger-policy` 域入口——与旧壳 list_insurers / create_insurer /
// update_insurer / delete_insurer 命令消费的同一域函数，不经 IPC。字典完整
// 性守卫（在用行唯一 / 空名拒绝 / NotFound）住域写入口单正文，本壳不自建；
// 旧壳写入后经 ledger:changed 参考失效重拉的失效语义在壳内对应物 = 视图
// 写成功回调自拉重读（保司不挂交易，交易失效信号不涉）；读连接是只读侧。
// ---------------------------------------------------------------------------

/// 保司字典全量行集（旧壳 `list_insurers` 命令同一执行协议）：含已删全量
///（在用 + 软删，name/created_at 升序）；在用/已删双分区拆分与「显示已删」
/// 只读分位归装配层纯面。读连接是成对建连的只读侧（ADR-0117）。
pub fn insurer_rows(db: &DbState, include_deleted: bool) -> Result<Vec<ledger_policy::Insurer>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_policy::list_insurers(&conn, include_deleted)
}

/// 创建保司（域公开写入口直调，旧壳 `create_insurer` 命令同一执行协议）：
/// trim 归一、空名拒绝与在用行同名码化拒绝（`insurer.already-exists`）在
/// 域内单正文（自持事务）。返回新保司 id。写连接是成对建连的写侧。
pub fn create_insurer(db: &DbState, input: ledger_policy::InsurerInput) -> Result<String> {
    write_entry_locked(db, |conn| ledger_policy::create_insurer(conn, input))
}

/// 改名保司（域公开写入口直调，旧壳 `update_insurer` 命令同一执行协议）：
/// 字段省略保持原值、空名拒绝与撞在用同名码化拒绝在域内单正文；不存在
///（或已软删）id → 码化 NotFound。写连接是成对建连的写侧。
pub fn update_insurer(
    db: &DbState,
    id: &str,
    input: ledger_policy::InsurerUpdateInput,
) -> Result<()> {
    write_entry_locked(db, |conn| ledger_policy::update_insurer(conn, id, input))
}

/// 软删除保司（域公开写入口直调，旧壳 `delete_insurer` 命令同一执行协议）：
/// 软删不占名字、存量保单引用照常显示（软删不可再被新选择由消费方校验）。
/// 不存在的 id → 码化 NotFound。写连接是成对建连的写侧。
pub fn delete_insurer(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_policy::delete_insurer(conn, id))
}

// ---------------------------------------------------------------------------
// 实物资产视图读写接缝（#2049）：实物资产清单（含在持估值合计同源快照）
// 直调 `ledger-physical-asset` 域入口——与旧壳 physicalAsset 命令族
//（listPhysicalAssets / createPhysicalAsset / updatePhysicalAsset /
// updatePhysicalAssetValuation / disposePhysicalAsset / deletePhysicalAsset）
// 消费的同一域函数，不经 IPC。域内单正文：建档估值必填（首条历史行）、
// 日期守卫（估值/处置拒绝未来、处置不早于购买）、金额与币种成对、软删保
// 留库行与估值历史、折算与在持合计（Amount 接缝当期汇率，缺汇率错误上
// 抛）——本壳不自建语义。失效信号回调注入在域写入口参数位保留（重读职责
// 归调用方，本壳传空闭包）：写成功后经信号族统一广播、视图订阅重读。写连
// 接是成对建连的写侧，读连接是只读侧（ADR-0117）。
// ---------------------------------------------------------------------------

/// 实物资产清单与在持估值合计（旧壳 `listPhysicalAssets` 命令同源）+ 状态
/// 筛选（在持 / 已处置，合法值解析归域）。折算与合计全在域读路径单点产
/// 出，本壳零二次聚合（V017 / ADR-0067 资产合计同纪律：算术不进壳）。
pub fn physical_asset_rows(
    db: &DbState,
    status_filter: Option<&str>,
) -> Result<ledger_physical_asset::PhysicalAssetList> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_physical_asset::list_physical_assets(&conn, status_filter)
}

/// 创建实物资产（域公开写入口直调，旧壳 `createPhysicalAsset` 命令同一执
/// 行协议）：校验归一化（名称 trim 非空、估值必填且 > 0、币种必填且存在、
/// 估值日期拒绝未来、购买价与币种成对）在域内，码化错误原样上抛。
pub fn create_physical_asset(
    db: &DbState,
    input: ledger_physical_asset::PhysicalAssetInput,
) -> Result<String> {
    write_entry_locked(db, |conn| {
        ledger_physical_asset::create_physical_asset(conn, input, &mut || {})
    })
}

/// 编辑实物资产档案（域公开写入口直调，旧壳 `updatePhysicalAsset` 命令同
/// 一执行协议）：全量替换名称与购买信息；估值不在入参内（结构性排除，
/// ADR-0064 只追加口径）。写连接是成对建连的写侧。
pub fn update_physical_asset(
    db: &DbState,
    id: &str,
    input: ledger_physical_asset::PhysicalAssetUpdateInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_physical_asset::update_physical_asset(conn, id, input, &mut || {})
    })
}

/// 追加估值历史行（域公开写入口直调，旧壳 `updatePhysicalAssetValuation`
/// 命令同一执行协议）：只追加不改写，当前值由读口径裁决为最新一条。
pub fn update_physical_asset_valuation(
    db: &DbState,
    id: &str,
    input: ledger_physical_asset::PhysicalAssetValuationInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_physical_asset::update_physical_asset_valuation(conn, id, input, &mut || {})
    })
}

/// 处置实物资产（域公开写入口直调，旧壳 `disposePhysicalAsset` 命令同一
/// 执行协议）：处置日期必填、处置价与币种成对可选纯记录（不进任何金额口
/// 径）；处置 = 状态标记非删除，档案保留可经已处置筛选回看。
pub fn dispose_physical_asset(
    db: &DbState,
    id: &str,
    input: ledger_physical_asset::PhysicalAssetDisposeInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_physical_asset::dispose_physical_asset(conn, id, input, &mut || {})
    })
}

/// 软删除实物资产（域公开写入口直调，旧壳 `deletePhysicalAsset` 命令同一
/// 执行协议）：`is_deleted=1` 保留库行与估值历史，不进清单与合计。
pub fn delete_physical_asset(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_physical_asset::delete_physical_asset(conn, id, &mut || {})
    })
}

// ---------------------------------------------------------------------------
// 设置接缝（#2053）：日志档位读写（基建 settings 共享接缝直调，运行期滤镜
// 经 crate::logging 接管）与 DataLocation 信息聚合 / 更改意图（基础设施
// data_location 直调——旧壳 get_log_level / set_log_level / get_data_location_
// info / submit_data_location_change / restore_default_data_location 命令消费
// 的同一域函数族，不经 IPC；码化错误沿旧壳同码同文案，用户可见语义一致）。
// 读写接缝零新口径：档位值域闭集在基建常量单点，默认档位回退口径与旧壳
// persisted_level 同形；DataLocation 校验三步 / 二选一/ 目录唯一性住基础设施
// 引导层，本壳不自建守卫语义（默认数据目录 = 启动参数账本目录——身份即目录，
// 立桩引导契约在设置面的延伸）。文件 IO 是阻塞面：调用方在 blocking 任务内
// 消费（卡流程同交易表单/备份先例）。写连接是成对建连的写侧。
// ---------------------------------------------------------------------------

/// 读取持久化日志档位（旧壳 `get_log_level` 命令同一执行协议）：缺 key /
/// 缺 `app_settings` 表（旧版本备份）回默认 info；库内残留闭集外字符串回
/// 默认 info 并告警（读路径不因坏值上抛）。读连接是只读侧。
pub fn log_level(db: &DbState) -> Result<String> {
    Ok(logging::persisted_level(db))
}

/// 设置日志档位（旧壳 `set_log_level` 命令同一执行协议）：闭集校验（错误码
/// [`CODE_LOG_LEVEL_INVALID`]〔旧壳同码同文案〕「日志等级非法：未知日志档位:
/// `x`」，未落库、未接管）→ 基建设置接缝落库（settings 单点收口，设置 KV
/// 不置脏——ADR-0032 豁免随接缝自带）→ 运行期滤镜接管（crate::logging，切档
/// 立即生效）。写连接是成对建连的写侧。
pub fn set_log_level(db: &DbState, level_str: &str) -> Result<()> {
    if !ledger_infra::settings::LOG_LEVEL_DIRECTIVES.contains(&level_str) {
        return Err(AppError::codedp(
            CODE_LOG_LEVEL_INVALID,
            format!("日志等级非法：未知日志档位: `{level_str}`"),
            &[level_str],
        ));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_infra::settings::set(
        &conn,
        ledger_infra::settings::SettingKey::LogLevel,
        &level_str,
    )?;
    // 闭集内指令恒可解析；解析失败走告警路径（滤镜未登记 / 换档失败不回写）。
    if let Some(level) = logging::parse_directive(level_str) {
        logging::set_level(level);
    }
    Ok(())
}

/// DataLocation 当前信息聚合（旧壳 `get_data_location_info` 命令同一执行协议
/// [`data_location::gather_info_from_boot`]）：引导记录缺失时按出厂降级呈现
/// 生效目录 = 默认目录（异常时序的诚实降级，旧壳同口径）。零库操作（意图读
/// 取是文件 IO：默认目录下引导文件）。
pub fn data_location_info(launch_dir: &Path) -> ledger_infra::db::data_location::DataLocationInfo {
    ledger_infra::db::data_location::gather_info_from_boot(launch_dir, boot_record().as_ref())
}

/// 提交更改位置意图（旧壳 `submit_data_location_change` 命令同一执行协议）：
/// 引导态预检（推迟搬迁窗口拦截）→ 三步校验与意图落盘全在基础设施
/// [`data_location::validate_and_commit`] 单正文（自动建目录 / 试写探针 /
/// 目标已有同名库二选一信号不做静默覆盖）。真实搬迁只发生在下次启动
/// （plan_boot 消费意图），文案必须讲清「下次启动生效」。文件 IO（建目录 /
/// 试写 / 落盘引导文件）为阻塞面。
pub fn submit_data_location_change(
    launch_dir: &Path,
    target_dir: &str,
    adopt_existing: bool,
) -> Result<ledger_infra::db::data_location::DataLocationChangeOutcome> {
    ledger_infra::db::data_location::ensure_relocation_settled(boot_record().as_ref())?;
    let trimmed = target_dir.trim();
    if trimmed.is_empty() {
        return Err(AppError::coded(
            "data-location.dir-required",
            "目标目录不能为空",
        ));
    }
    ledger_infra::db::data_location::validate_and_commit(
        launch_dir,
        Path::new(trimmed),
        adopt_existing,
    )
}

/// 恢复默认位置（旧壳 `restore_default_data_location` 命令同一执行协议）：
/// 目标 = 本壳默认数据目录（启动参数目录），校验与写意图机制与更改完全相同
/// （二选一信号 / 目录唯一性拒绝随同一入口）。
pub fn restore_default_data_location(
    launch_dir: &Path,
    adopt_existing: bool,
) -> Result<ledger_infra::db::data_location::DataLocationChangeOutcome> {
    ledger_infra::db::data_location::ensure_relocation_settled(boot_record().as_ref())?;
    ledger_infra::db::data_location::validate_and_commit(launch_dir, launch_dir, adopt_existing)
}

// ---------------------------------------------------------------------------
// 数据安全与连接接缝（#2054 设置②）：备份管理全量面（列表 / 清理 / 自动备份
// 状态 / 恢复）、加密状态与开启、多端同步（状态 / 立即同步 / 通道配置 /
// 测试连接 / 检查点 / 引导）与账本注册表（清单 / 新建 / 切换 / 改名 / 移除）
// 直调域与基础设施入口——与旧壳 create_backup、restore_backup、list_backups、
// prune_backups、get_auto_backup_state、set_auto_backup_enabled、get_backup_meta、
// get_encryption_status、enable_encryption、get_sync_status、sync_now、
// get_parked_ops、get/set_sync_channel_config、test_sync_channel_connection、
// get/publish_sync_checkpoint、bootstrap_sync_from_channel、list/create/switch/
// rename/remove_book 命令消费的同一域函数，不经 IPC。写连接是成对建连的写
// 侧，读连接是只读侧（ADR-0117）。同步写语义不经本壳面（同步域编排照旧），
// 本壳只做触发与呈现（issue #2054 实施决策）。
// ---------------------------------------------------------------------------

use ledger_backup::{BackupFileInfo, BackupMetaSummary, PruneResult, RestoreResult};
use ledger_sync_engine::{SyncChannelConfig, SyncRoundReport};

/// 当前生效账本库文件路径（旧壳 `active_db_path` 的本壳对应面）：引导记录
/// 登记的生效目录（设置卡数据位置的同一读源）为单一来源；未登记（极端时序）
/// 回退启动参数目录（active_db_path 的 default_data_dir 兜底同形）。
fn db_path_for_data_safety() -> Result<PathBuf> {
    let dir = match boot_record() {
        Some(record) => record.db_dir,
        None => crate::app::book_dir()
            .ok_or_else(|| AppError::coded(CODE_SESSION_MISSING, "内部错误：账本库会话缺失"))?,
    };
    Ok(dir.join(ledger_infra::db::data_location::DB_FILE_NAME))
}

/// 当前活动账本标识（旧壳 `active_book_id` 的本壳对应面）：引导快照注册表的
/// 活动指针；注册表不可用（损坏回退）回 `None`——钥匙串条目分域与多端同步
/// 门禁的同一兜底口径。
fn active_book_id() -> Option<String> {
    boot_record()
        .as_ref()
        .and_then(|record| record.registry.as_ref())
        .map(|registry| registry.active_id.clone())
}

// —— 备份管理（备份历史、滚动清理、自动备份状态与恢复） ——

/// 受管备份行集（旧壳 `list_backups` 命令同一执行协议）：备份目录内当前账本
/// （指纹 × 设备）可归属的产物，按新→旧排序。作用域按 **账本指纹** 分域
/// （ADR-0144 决策 3，[`ledger_backup::BackupScope::of_conn`]）；指纹未固化
/// （空库）= 空列表（本账尚无任何可归属产物）。读连接是只读侧。
pub fn backup_rows(db: &DbState, dir: &Path) -> Result<Vec<BackupFileInfo>> {
    let conn = conn_lock(&db.read_conn)?;
    match ledger_backup::BackupScope::of_conn(&conn)? {
        Some(scope) => ledger_backup::list_managed_backups(dir, Some(&scope)),
        None => Ok(Vec::new()),
    }
}

/// 滚动清理（旧壳 `prune_backups` 命令同一执行协议）：把（本指纹 × 本设备）
/// 桶内超额产物修剪到 `keep`。保留上限非法（0）报旧壳同码
/// `backup.keep-invalid`（UI 输入钳 1–100，码化拒绝是口径兜底）。文件删除是
/// 阻塞 IO，调用方在 blocking 任务内消费；删除结果由调用方触发列表重读（旧
/// 壳 `ledger:backups-changed` 信号的壳内对应物）。
pub fn prune_backups(db: &DbState, dir: &Path, keep: usize) -> Result<PruneResult> {
    if keep == 0 {
        return Err(AppError::coded(
            "backup.keep-invalid",
            format!("备份保留上限非法: {keep}"),
        ));
    }
    let conn = conn_lock(&db.read_conn)?;
    match ledger_backup::BackupScope::of_conn(&conn)? {
        Some(scope) => ledger_backup::prune_managed_backups(dir, keep, Some(&scope)),
        None => Ok(PruneResult {
            kept: 0,
            deleted: Vec::new(),
            failed: Vec::new(),
        }),
    }
}

/// 本机设备标识（旧壳 `get_device_id` 命令同一执行协议）：清理确认弹窗待删
/// 数量「本指纹 × 本设备」桶判定的比对基准；不可得（attached 侧异常）=
/// `None`，保守不误报（旧壳同口径）。
pub fn backup_device_id(db: &DbState) -> Option<String> {
    let conn = conn_lock(&db.read_conn).ok()?;
    ledger_backup::source_device_id(&conn)
}

/// 自动备份状态（旧壳 `get_auto_backup_state` 命令同一执行协议）：开关 +
/// 上次自动备份时刻 + 连续失败次数与提示态（阈值判定归域单点
/// [`ledger_backup::failure_alerting`]，本壳不复刻）。备份目录是本壳平台
/// 存储偏好（旧壳 localStorage 同位），不经库。
pub fn auto_backup_state(db: &DbState) -> Result<ledger_backup::AutoBackupState> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_backup::get_state(&conn)
}

/// 自动备份开关连续失败提示态（与 [`auto_backup_state`] 同源的阈值投影单点）。
pub fn auto_backup_failure_alerting(consecutive_failures: u32) -> bool {
    ledger_backup::failure_alerting(consecutive_failures)
}

/// 设置自动备份开关（旧壳 `set_auto_backup_enabled` 命令同一执行协议）：写
/// `app_settings` 经基建设置接缝（设置 KV 置脏豁免，ADR-0032 随接缝自带）。
/// 开关由自动备份调度消费（宿主已随 #2078 落地：调度线程每轮读库内状态，
/// 关闭即静默跳过，三触发入口统一尊重），写入是跨壳共享的库内状态。
pub fn set_auto_backup_enabled(db: &DbState, enabled: bool) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_infra::settings::set(
        &conn,
        ledger_infra::settings::SettingKey::AutoBackupEnabled,
        &enabled,
    )
}

/// 备份包元数据摘要（旧壳 `get_backup_meta` 命令同一执行协议）：恢复确认
/// 弹窗的加密标记与来源标记（跨模式警告与口令输入判定）。纯文件探测。
pub fn backup_meta(path: &Path) -> Result<BackupMetaSummary> {
    ledger_backup::probe_backup_meta(path)
}

/// 从备份文件恢复（旧壳 `restore_backup` 命令同一执行协议）：安全备份 →
/// 整库替换 → 读连接占位化。目标库路径 = 引导生效目录（DataLocation 解析的
/// 单点，与旧壳 effective_db_dir 同语义），恢复安全备份落应用数据根
/// （RestoreSafetyBackup 既有语义；本壳数据根与旧壳同一 XDG 拼接根）。恢复
/// 期间持写连接锁（阻塞并发写，旧壳同口径）；成功后立刻占位化读连接（旧
/// inode 读句柄只给陈旧数据，issue #1280 / ADR-0117 决策 3），本壳无原位重
/// 引导机制，重启语义由状态条提示承担（偏差见交付报告）。任务无连接外格
/// 外持锁面：全部步骤在同一 blocking 任务内。
pub fn restore_backup(
    db: &DbState,
    backup_path: &Path,
    passphrase: Option<&str>,
) -> Result<RestoreResult> {
    let db_path = db_path_for_data_safety()?;
    let safety_dir = crate::cross_book::default_data_dir()
        .ok_or_else(|| AppError::Io("应用数据根不可解析，无法保存恢复安全备份".to_owned()))?;
    // 恢复期间持有写连接锁（阻塞本地 HTTP/IPC 并发写的同位语义：本壳的并发
    // 写面 = 域直调写路径 + 同步轮次，同吃这把锁）。
    let _guard = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let expected = ledger_backup::expected_schema_version()?;
    let result =
        ledger_backup::restore_db_from(backup_path, &db_path, &safety_dir, expected, passphrase);
    if result.is_ok() {
        // 文件替换成功：读连接立即占位化（同旧壳 detach_read_conn；失败仅记
        // 日志，不改变恢复结果，旧读连接占位化失败的行为同口径）。
        if let Err(e) = db.placeholderize_read_conn() {
            tracing::error!(error = %e, "恢复后读连接占位化失败，重启前读命令将持续报错");
        }
    }
    result
}

// —— 加密（状态与整库开启；关闭/改主口令随密文库引导解锁票开门） ——

/// 加密状态（旧壳 `get_encryption_status` 命令同一执行协议）：库文件头探测
/// （文件即真相，ADR-0075 决策 4）。锁定门在本壳恒不存在——NSH 只引导未
/// 加密库，锁定态随解锁向导体（界外另票）开门。
pub fn encryption_status() -> Result<bool> {
    let file_encrypted = ledger_infra::db::encryption::probe_file_kind(&db_path_for_data_safety()?)?
        == ledger_infra::db::encryption::DbFileKind::Encrypted;
    Ok(file_encrypted)
}

/// 开启加密（旧壳 `enable_encryption` 命令同一执行协议）：当前明文库整库
/// 一次性转换为密文库（转换与原子性语义全在基础设施
/// [`encryption::enable_encryption_for_file`] 单正文，失败原库原样保留）。
/// 文件替换成功后读连接立即占位化（旧读连接只给陈旧数据，同恢复通道）。
/// 本壳无原位重引导：转换完成后下次启动将呈现「密文库暂不支持」启动失败
/// （解锁向导界外另票），状态反馈须如实携带该后果（与旧壳自动重启的偏差
/// 见交付报告）。
pub fn enable_encryption(db: &DbState, passphrase: &str) -> Result<()> {
    let db_path = db_path_for_data_safety()?;
    ledger_infra::db::encryption::enable_encryption_for_file(&db_path, passphrase)?;
    if let Err(e) = db.placeholderize_read_conn() {
        tracing::error!(error = %e, "整库加密转换后读连接占位化失败，重启前读命令将持续报错");
    }
    Ok(())
}

/// 主口令验证（信封模式解析的消费面，与旧壳 `resolve_passphrase` 同序）。
pub fn verify_source_passphrase_for_sync(db_path: &Path, passphrase: &str) -> Result<()> {
    ledger_infra::db::encryption::verify_source_passphrase(db_path, passphrase)
}

/// 本机已记住主口令的读取（信封模式解析的钥匙串回退面；发布构建的生物门
/// 在 v1 非 macOS 平台不支持，本壳按 infra 既有语义只投 Found/NotFound 路径）。
pub fn remembered_sync_passphrase(book: Option<&str>) -> Result<Option<String>> {
    match ledger_infra::db::passphrase_cache::load(book)? {
        ledger_infra::db::passphrase_cache::CacheLoad::Found(pass) => Ok(Some(pass)),
        _ => Ok(None),
    }
}

// —— 多端同步（状态 / 立即同步 / 通道配置 / 测试连接 / 检查点 / 引导） ——

/// 同步状态投影（旧壳 `get_sync_status` 命令响应体的本壳消费面；设备标识
/// 不在本壳呈现面，投影不携带——偏差见交付报告）。
#[derive(Clone, Debug, PartialEq)]
pub struct SyncStatusSeam {
    /// 通道是否已配置。
    pub channel_configured: bool,
    /// 上次成功同步时刻（UTC ISO；从未同步 = None）。
    pub last_sync_at: Option<String>,
    /// 挂起 op 数量。
    pub parked_count: usize,
    /// 本库是否密文形态（信封模式显性提示依据）。
    pub library_encrypted: bool,
}

/// 读取同步状态（旧壳 `get_sync_status` 命令同一执行协议）：通道配置在位、
/// 上次成功同步时刻、挂起数量与库加密形态（文件头探测）。读路径零副作用；
/// 缺 key / 缺表回默认。
pub fn sync_status(db: &DbState) -> Result<SyncStatusSeam> {
    let conn = conn_lock(&db.read_conn)?;
    let channel_configured = ledger_sync_engine::configured_channel(&conn)?.is_some();
    let last_sync_at = ledger_infra::settings::get(
        &conn,
        ledger_infra::settings::SettingKey::SyncLastSyncAt,
        None,
    )?;
    let parked_count = ledger_sync_engine::parked_ops(&conn)?.len();
    drop(conn);
    let library_encrypted = encryption_status()?;
    Ok(SyncStatusSeam {
        channel_configured,
        last_sync_at,
        parked_count,
        library_encrypted,
    })
}

/// 挂起操作行集（旧壳 `get_parked_ops` 命令同一执行协议）：本轮挂起的明细
/// 挂接面；呈现用已渲染中文详情（[`ledger_sync_engine::ParkedOp::message`]）。
pub fn parked_rows(db: &DbState) -> Result<Vec<ledger_sync_engine::ParkedOp>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_sync_engine::parked_ops(&conn)
}

/// 通道配置读取（旧壳 `get_sync_channel_config` 命令同一执行协议）：未配置
/// 回 `None`；缺 key / 缺表按设置域读口径回默认。
pub fn sync_channel_config(db: &DbState) -> Result<Option<SyncChannelConfig>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_sync_engine::configured_channel(&conn)
}

/// 保存通道配置（旧壳 `set_sync_channel_config` 命令同一执行协议）：端点非
/// https 拒绝——用户输入门与旧壳命令层同位；同步空间空白归默认空间）→
/// 凭据构库校验（构库单点不发网络请求）→ 经设置接缝落 `app_settings`。
/// 两步校验通过才写入；失败零写入（不产生半配置态）。写连接是写侧。
pub fn save_sync_channel_config(db: &DbState, mut config: SyncChannelConfig) -> Result<()> {
    let endpoint = config.endpoint.trim().to_owned();
    if !endpoint.is_empty() && !endpoint.to_ascii_lowercase().starts_with("https://") {
        return Err(AppError::coded(
            "sync-channel.endpoint-insecure",
            "同步通道端点必须使用 https 地址",
        ));
    }
    config.endpoint = endpoint;
    if config.space_id.trim().is_empty() {
        config.space_id = ledger_sync_engine::trigger::DEFAULT_SPACE_ID.to_owned();
    }
    ledger_sync_engine::build_channel(&config)?;
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_infra::settings::set(
        &conn,
        ledger_infra::settings::SettingKey::SyncChannelConfig,
        &config,
    )
}

/// 保存前「测试连接」（旧壳 `test_sync_channel_connection` 命令同一执行协
/// 议）：当前表单尚未落库的配置做一次只读探针，零持久化副作用。用户输入门
/// （非 https 拒绝）与构库校验同保存路径同一单点。
pub fn test_sync_connection(config: &SyncChannelConfig) -> Result<()> {
    let endpoint = config.endpoint.trim().to_owned();
    if !endpoint.is_empty() && !endpoint.to_ascii_lowercase().starts_with("https://") {
        return Err(AppError::coded(
            "sync-channel.endpoint-insecure",
            "同步通道端点必须使用 https 地址",
        ));
    }
    let mut probe = config.clone();
    probe.endpoint = endpoint;
    ledger_sync_engine::probe_channel(&probe)
}

/// 轮次连接源（ADR-0120 决策 4 的本壳形态）：把 DbState 写连接槽包成轮次
/// 分段接缝——每个数据库段短暂取一次连接、用完即还，网络段不消费连接
/// （与旧壳 MainConnSegments 同形状）；持锁时长探针与 #1276 守门口径一致。
struct ShellRoundConn {
    conn: std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
    key: u64,
}

impl ledger_sync_engine::RoundConn for ShellRoundConn {
    fn with_connection<R, F>(
        &self,
        _segment: ledger_sync_engine::ConnSegment,
        use_connection: F,
    ) -> Result<R>
    where
        F: FnOnce(&rusqlite::Connection) -> Result<R>,
    {
        let hold_started = std::time::Instant::now();
        let guard = self.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let result = use_connection(&guard);
        ledger_infra::db::probe_lock_hold(hold_started.elapsed());
        result
    }

    fn round_key(&self) -> u64 {
        self.key
    }
}

/// 手动同步一轮（旧壳 `sync_now` 命令同一执行协议）：通道读取（短取锁）→
/// 构库 → 主口令解析（明文库明文直通；密文库凭显式口令或本机缓存，先验证
/// 后封包）→ 轮次编排归域（在途互斥 / 幂等重放 / 挂起语义在域内单点）。
/// 成功后记会话信封（旧壳 SessionEnvelope::remember 同语义保留）。「上次
/// 成功同步时刻」由域内整体裁决点落库
/// （旧壳同点）；报告 `applied > 0` 的失效信号由视图层广播（本函数零信号
/// 副作用）。
pub fn sync_now(db: &DbState, passphrase: Option<&str>) -> Result<SyncRoundReport> {
    let db_path = db_path_for_data_safety()?;
    let book = active_book_id();
    // 通道读取 + 未配置 / 账本門禁（短取读连接）。
    let config = {
        let conn = conn_lock(&db.read_conn)?;
        ledger_sync_engine::configured_channel(&conn)?
            .ok_or_else(ledger_sync_engine::trigger::not_configured_error)?
    };
    if book.is_none() {
        return Err(ledger_sync_engine::trigger::book_unavailable_error());
    }
    let channel = ledger_sync_engine::build_channel(&config)?;
    let holder = passphrase_holder(db_path.as_path(), book.as_deref(), passphrase)?;
    let mode = match &holder {
        Some(holder) => ledger_sync_engine::EnvelopeMode::Encrypted { passphrase: holder },
        None => ledger_sync_engine::EnvelopeMode::Plaintext,
    };
    let locks = ShellRoundConn {
        conn: std::sync::Arc::clone(&db.conn),
        key: ledger_sync_engine::connection_round_key(&db.conn),
    };
    let report = ledger_sync_engine::run_round_once(&locks, &channel, &mode)?;
    // 成功轮次记入本会话形态（域语义保留：手动同步后不再需要重新触口令）。
    match &holder {
        Some(holder) => ledger_sync_engine::SessionEnvelope::remember(
            ledger_sync_engine::SessionEnvelope::Encrypted(holder.clone()),
        ),
        None => ledger_sync_engine::SessionEnvelope::remember(
            ledger_sync_engine::SessionEnvelope::Plaintext,
        ),
    }
    Ok(report)
}

/// 主口令解析（旧壳 `resolve_passphrase` 同序单点）：明文库回 `None`（明文
/// 直通）；密文库回 `Some`（显式口令优先，其次本机缓存，均不可得报
/// `sync-channel.passphrase-required`），先验证后封包（错误口令封出的段对端
/// 无法解封，必须在上传前拦下——`encryption.passphrase-incorrect` 同码）。
fn passphrase_holder(
    db_path: &Path,
    book: Option<&str>,
    passphrase: Option<&str>,
) -> Result<Option<String>> {
    if ledger_infra::db::encryption::probe_file_kind(db_path)?
        != ledger_infra::db::encryption::DbFileKind::Encrypted
    {
        return Ok(None);
    }
    let passphrase = match passphrase {
        Some(passphrase) => passphrase.to_owned(),
        None => remembered_sync_passphrase(book)?.ok_or_else(|| {
            AppError::coded(
                "sync-channel.passphrase-required",
                "同步通道上有加密数据，需要主口令才能读取",
            )
        })?,
    };
    ledger_infra::db::encryption::verify_source_passphrase(db_path, &passphrase)?;
    Ok(Some(passphrase))
}

/// 通道检查点指针（旧壳 `get_sync_channel_checkpoint` 命令同一执行协议）：
/// 只读 manifest 不下载快照体，引导向导首步的预检。通道未配置报旧壳同码
/// `sync-channel.not-configured`。网络等待不消费连接（配置短取读锁）。
pub fn sync_channel_checkpoint(
    db: &DbState,
) -> Result<Option<ledger_sync_engine::CheckpointPointer>> {
    let config = {
        let conn = conn_lock(&db.read_conn)?;
        ledger_sync_engine::configured_channel(&conn)?
            .ok_or_else(ledger_sync_engine::trigger::not_configured_error)?
    };
    let channel = ledger_sync_engine::build_channel(&config)?;
    channel.checkpoint_pointer()
}

/// 发布检查点（旧壳 `publish_sync_checkpoint` 命令同一执行协议）：快照产出
/// （写连接锁只盖这一步，位点与快照同刻成对）→ 封包与三次通道往返在锁外。
/// 本地数据零变化（零信号，通道操作而非账本写入，旧壳同口径）。
pub fn publish_sync_checkpoint(
    db: &DbState,
    passphrase: Option<&str>,
) -> Result<SyncCheckpointPublished> {
    let db_path = db_path_for_data_safety()?;
    let book = active_book_id();
    let config = {
        let conn = conn_lock(&db.read_conn)?;
        ledger_sync_engine::configured_channel(&conn)?
            .ok_or_else(ledger_sync_engine::trigger::not_configured_error)?
    };
    if book.is_none() {
        return Err(ledger_sync_engine::trigger::book_unavailable_error());
    }
    let channel = ledger_sync_engine::build_channel(&config)?;
    let holder = passphrase_holder(db_path.as_path(), book.as_deref(), passphrase)?;
    let mode = match &holder {
        Some(holder) => ledger_sync_engine::EnvelopeMode::Encrypted { passphrase: holder },
        None => ledger_sync_engine::EnvelopeMode::Plaintext,
    };
    // 锁内段：快照产出（主连接互斥，随语句块立即释放）。
    let checkpoint = {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        ledger_sync_engine::create_checkpoint(&conn)?
    };
    // 锁外段：封包（KDF）与通道网络往返。
    let pointer = channel.upload_checkpoint(&checkpoint, &mode)?;
    Ok(SyncCheckpointPublished {
        generation: pointer.generation,
        size: pointer.size,
        plaintext_mode: mode.is_plaintext(),
    })
}

/// 发布结果（旧壳 `SyncCheckpointPublished` 同形；视图提示的数据面）。
#[derive(Clone, Debug)]
pub struct SyncCheckpointPublished {
    /// 本次发布的代数。
    pub generation: i64,
    /// 密文字节数。
    pub size: u64,
    /// 明文模式标记（快照明文上通道，界面显著提示依据）。
    pub plaintext_mode: bool,
}

/// 引导主连接段接缝（域 [`ledger_sync_engine::BootstrapConnSegments`] 的本壳
/// 唯一实现，与旧壳 MainConnSegments 同形）：每段短取一次写连接——锁失败映
/// 射与持锁时长探针内化此处。
struct ShellBootstrapSegments {
    conn: std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
}

impl ledger_sync_engine::BootstrapConnSegments for ShellBootstrapSegments {
    fn with_conn<T>(
        &self,
        use_conn: impl FnOnce(&mut rusqlite::Connection) -> Result<T>,
    ) -> Result<T> {
        let hold_started = std::time::Instant::now();
        let mut conn = self.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let result = use_conn(&mut conn);
        ledger_infra::db::probe_lock_hold(hold_started.elapsed());
        result
    }
}

/// 从通道检查点引导（旧壳 `bootstrap_sync_from_channel` 命令同一执行协议）：
/// 拉取通道当前检查点并整库换入本机——「加入即新库」的显式向导动作，编排
/// 全在域单点 [`ledger_sync_engine::bootstrap_from_channel`]（前置守卫、拉
/// 取、复验、信封形态对齐、整库换入、簿记清理与转密文决策在域内），本壳只
/// 实现段接缝。整库换入后本壳无原位重引导，重启语义由状态条提示承担。
pub fn bootstrap_sync_from_channel(
    db: &DbState,
    passphrase: Option<&str>,
) -> Result<ledger_sync_engine::BootstrapOutcome> {
    let db_path = db_path_for_data_safety()?;
    let config = {
        let conn = conn_lock(&db.read_conn)?;
        ledger_sync_engine::configured_channel(&conn)?
            .ok_or_else(ledger_sync_engine::trigger::not_configured_error)?
    };
    let channel = ledger_sync_engine::build_channel(&config)?;
    let segments = ShellBootstrapSegments {
        conn: std::sync::Arc::clone(&db.conn),
    };
    ledger_sync_engine::bootstrap_from_channel(&segments, &db_path, &channel, passphrase)
}

// —— 账本注册表（清单 / 新建 / 切换 / 改名 / 移除；账本切换卡消费面） ——

/// 账本清单聚合（旧壳 `list_books` 命令同一执行协议）：登记清单、活动指针、
/// 登记变更可用性与回退警示。清单读注册表**最新落盘态**（登记变更落盘后立
/// 即可见）；本壳默认数据目录 = 启动参数账本目录（#2053 身份即目录的同一
/// 契约），聚合与数据位置卡同归口。
pub fn book_list(launch_dir: &Path) -> ledger_infra::db::book_registry::BookListInfo {
    ledger_infra::db::data_location::gather_book_list(launch_dir, boot_record().as_ref())
}

/// 新建账本（旧壳 `create_book` 命令同一执行协议）：登记变更预检 → 引导内核
/// 登记（自动建子目录、查重拒绝、首次登记落新格式全在域内单正文）。返回
/// 登记后的账本条目。
pub fn create_book(launch_dir: &Path, name: &str) -> Result<ledger_infra::db::book_registry::Book> {
    ledger_infra::db::data_location::mutable_registry(boot_record().as_ref())?;
    ledger_infra::db::book_registry::create_book_entry(launch_dir, name)
}

/// 切换活动账本（旧壳 `switch_book` 命令同一执行协议）：登记变更预检 → 写
/// 活动指针落盘。重引导（进目标账本）本壳无原位机制——重启语义由状态条
/// 提示承担（旧壳 restart_app 的偏差见交付报告）。
pub fn switch_book(launch_dir: &Path, id: &str) -> Result<ledger_infra::db::book_registry::Book> {
    ledger_infra::db::data_location::mutable_registry(boot_record().as_ref())?;
    ledger_infra::db::book_registry::switch_active_book(launch_dir, id)
}

/// 改账本展示名（旧壳 `rename_book` 命令同一执行协议）：只动注册表元数据，
/// 目录与库文件零变化。
pub fn rename_book(
    launch_dir: &Path,
    id: &str,
    name: &str,
) -> Result<ledger_infra::db::book_registry::Book> {
    ledger_infra::db::data_location::mutable_registry(boot_record().as_ref())?;
    ledger_infra::db::book_registry::rename_book_entry(launch_dir, id, name)
}

/// 移除账本（旧壳 `remove_book` 命令同一执行协议）：仅摘登记，目录与库文件
/// 原样保留（可重新登记找回）；活动账本不可移除在域内核拒绝。
pub fn remove_book(launch_dir: &Path, id: &str) -> Result<()> {
    ledger_infra::db::data_location::mutable_registry(boot_record().as_ref())?;
    ledger_infra::db::book_registry::remove_book_entry(launch_dir, id)
}
