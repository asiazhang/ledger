//! 域栈引导（issue #2017 / ADR-0145）：打开未加密账本库，读交易列表页、交易
//! 搜索与参考数据（币种字典 / 在用分类 / 可见账户 / 含软删商户——交易表格
//! 首屏、issue #2023 搜索接入与派生行形态的装配消费面），#2033 起兼预算
//! 读写、#2034 起兼交易表单族参考数据与基础收支写路径。
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

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use ledger_categories::Category;
use ledger_currencies::Currency;
use ledger_infra::db::boot::{BOOT_DB_UNREADABLE, BootDisposition};
use ledger_infra::db::data_location::DB_FILE_NAME;
use ledger_infra::db::{DbState, open_db_in};
use ledger_infra::error::{AppError, Result};
use ledger_transaction::{TransactionListResult, TransactionSearchResult};

use crate::assembly::PAGE_SIZE;
use crate::wiring;

/// 密文库不支持（立桩范围外）的稳定错误码。
pub const CODE_ENCRYPTED_UNSUPPORTED: &str = "nsh-shell.encrypted-unsupported";

/// 账本库文件缺失的稳定错误码。
pub const CODE_BOOK_MISSING: &str = "nsh-shell.book-missing";

/// 账本库会话缺失的稳定错误码（写路径在会话就位前执行的不可达兜底）。
pub const CODE_SESSION_MISSING: &str = "nsh-shell.session-missing";

/// 文件保存对话框无法打开的稳定错误码（#2038：Linux 无文件选择 portal 等
/// 平台能力缺失；平台错误原文随文案透传，呈现面归视图层）。
pub const CODE_FILE_DIALOG_FAILED: &str = "nsh-shell.file-dialog-failed";

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
/// 序列与旧壳明文启动路径同形：接缝接线（先于任何建库，幂等）→ 处置判定 →
/// （外来形态明文库先归一化，原库失败保持原样）→ 成对建连（迁移 + 只读连接 +
/// 同步元数据库挂载，全部收在 [`open_db_in`] 单点）。
pub fn boot_ledger(book_dir: &Path) -> Result<DbState> {
    wiring::install_domain_seams();
    let db_path = book_dir.join(DB_FILE_NAME);
    if !db_path.is_file() {
        return Err(AppError::codedp(
            CODE_BOOK_MISSING,
            format!(
                "账本库文件不存在：{}（立桩期请传入已有账本的目录，不新建账本）",
                db_path.display()
            ),
            &[&db_path.to_string_lossy()],
        ));
    }
    match ledger_infra::db::boot::classify_for_boot(&db_path)? {
        BootDisposition::OpenPlaintext => open_db_in(book_dir),
        BootDisposition::NormalizePlaintext => {
            normalize_foreign_plaintext(&db_path)?;
            open_db_in(book_dir)
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
/// 刷新接缝（[`crate::wiring`]）在既有事务内执行。写连接是成对建连的写侧。
/// 不存在的 id 返回码化 NotFound（并发已删的呈现面归视图层）。
pub fn delete_transaction(db: &DbState, id: &str) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_transaction::delete(&conn, id)
}

// ---------------------------------------------------------------------------
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
/// 接缝在既有事务内执行。返回写入证据（id + 是否即建商户）。写连接是成对
/// 建连的写侧。
pub fn create_transaction(
    db: &DbState,
    input: ledger_transaction::TransactionInput,
) -> Result<ledger_transaction::TransactionWrite> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_transaction::create(&conn, input)
}

/// 按 id 全字段替换交易（域公开写入口直调，旧壳 `update_transaction` 命令
/// 同一执行协议；与 HTTP `PUT /api/v1/transactions/{id}` 同一行为层权威）：
/// 修改编排入口 [`ledger_transaction::update`]。返回「是否即建商户」证据。
/// 不存在的 id 报码化 NotFound。写连接是成对建连的写侧。
pub fn update_transaction(
    db: &DbState,
    id: &str,
    input: ledger_transaction::TransactionInput,
) -> Result<ledger_infra::signals::WriteEvidence> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_transaction::update(&conn, id, input)
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
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_budget::crud::create_budget(&conn, input)
}

/// 编辑预算金额与周期（域公开写入口直调，旧壳 `update_budget` 命令同一执行
/// 协议；分类不可改，改法为删旧建新）。写连接是成对建连的写侧。
pub fn update_budget(
    db: &DbState,
    id: &str,
    amount_cents: i64,
    period: Option<ledger_budget::BudgetPeriod>,
) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_budget::crud::update_budget(&conn, id, amount_cents, period)
}

/// 删除预算（软删；域公开写入口直调，旧壳 `delete_budget` 命令同一执行协议）。
/// 不存在的 id 报码化 NotFound（并发已删的呈现面归视图层）。写连接是成对建连
/// 的写侧。
pub fn delete_budget(db: &DbState, id: &str) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_budget::crud::delete_budget(&conn, id)
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
