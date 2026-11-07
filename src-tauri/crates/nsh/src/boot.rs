//! 域栈引导（issue #2017 / ADR-0145）：打开未加密账本库，读交易列表页、交易
//! 搜索与参考数据（币种字典 / 在用分类 / 可见账户 / 含软删商户——交易表格
//! 首屏、issue #2023 搜索接入与派生行形态的装配消费面），#2033 起兼预算
//! 读写、#2034 起兼交易表单族参考数据与基础收支写路径、#2053 起兼设置接缝
//! （日志档位 / DataLocation 信息聚合与更改意图 / 引导记录）。
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

/// 启动读路径共用的启动会话锚：本壳的「默认数据目录」 = 启动参数账本
/// 目录（立桩契约的延续：身份即目录，目录即账本世界入口）；#2053 起
/// [`ledger_infra::db::boot::plan_boot`] 在其上消费账本注册表——
/// DataLocation 更改意图（搬迁 / 接管）在下次启动（同目录参数）生效，与
/// 设置卡写出的意图同接缝（旧壳 plan_boot 消费面同形）。
pub fn boot_ledger(book_dir: &Path) -> Result<DbState> {
    wiring::install_domain_seams();
    let plan = ledger_infra::db::boot::plan_boot(book_dir);
    let db_path = plan.boot.db_dir.join(DB_FILE_NAME);
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
            normalize_foreign_plaintext(&db_path)?;
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
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_item::create_item(&conn, input, &mut || {})
}

/// 按 id 修改物品（域公开写入口直调，旧壳 `update_item` 命令同一执行协议）：
/// 换关带出 / 不可达状态守卫在域内。写连接是成对建连的写侧。
pub fn update_item(db: &DbState, id: &str, input: ledger_item::ItemInput) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_item::update_item(&conn, id, input, &mut || {})
}

/// 处置物品（域公开写入口直调，旧壳 `dispose_item` 命令同一执行协议）：日期
/// / 残值守卫在域内；已处置再次处置 = 修正处置信息。写连接是成对建连的写侧。
pub fn dispose_item(db: &DbState, id: &str, input: ledger_item::ItemDisposeInput) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_item::dispose_item(&conn, id, input, &mut || {})
}

/// 软删物品（域公开写入口直调，旧壳 `delete_item` 命令同一执行协议）。写连
/// 接是成对建连的写侧。
pub fn delete_item(db: &DbState, id: &str) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_item::delete_item(&conn, id, &mut || {})
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
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_accounts::create_account(&conn, input)
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
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    if input.is_archived == Some(true) {
        ledger_savings_goal::ensure_account_can_archive(&conn, id)?;
    }
    ledger_accounts::update_account(&conn, id, input)
}

/// 删除账户（软删；域公开写入口直调，旧壳 `delete_account` 命令同一执行协议）：
/// 目标域守卫在入口先行（「删账户命令先经目标域校验」，旧壳命令壳同位——账户
/// 域不反向依赖目标域，ADR-0133 决策 4）：在用目标的专属账户被码化拒绝，普通
/// 账户照常放行。写连接是成对建连的写侧。
pub fn delete_account(db: &DbState, id: &str) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_savings_goal::ensure_account_not_goal_bound(&conn, id)?;
    ledger_accounts::delete_account(&conn, id)
}

/// 归档账户（幂等；域入口直调，旧壳 `archive_account` 命令同一执行协议）：
/// 绑定目标的账户被码化拒绝（守卫住目标域，旧壳命令壳同位）。写连接是成对
/// 建连的写侧。
pub fn archive_account(db: &DbState, id: &str) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_savings_goal::ensure_account_can_archive(&conn, id)?;
    ledger_accounts::set_account_archived(&conn, id, true)
}

/// 恢复账户（归档的唯一回退动作；域入口直调，旧壳 `unarchive_account` 命令
/// 同一执行协议）。写连接是成对建连的写侧。
pub fn unarchive_account(db: &DbState, id: &str) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_accounts::set_account_archived(&conn, id, false)
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
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_accounts::adjust_account_balance(&conn, id, input)
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
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_scheduled::create_plan(&conn, input)
}

/// 变更计划状态（域公开写入口直调，旧壳 `updateScheduledTransactionStatus`
/// 命令同一执行协议）：守护裁决 + 取消级联在域内（opcode 单点）。写连接是
/// 成对建连的写侧。
pub fn update_scheduled_plan_status(
    db: &DbState,
    id: &str,
    new_status: ledger_scheduled::ScheduledStatus,
) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_scheduled::update_plan_status(&conn, id, new_status)
}

/// 编辑订阅计划的非金额字段（域公开写入口直调，旧壳
/// `updateScheduledSubscription` 命令同一执行协议）：金额哨兵拒绝、商户保持
/// 历史引用语义与「保单缴费协议不挂商户」裁决全在域内单点。写连接是成对
/// 建连的写侧。
pub fn update_scheduled_subscription(
    db: &DbState,
    input: ledger_scheduled::UpdateSubscriptionInput,
) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_scheduled::update_subscription(&conn, input)
}

/// 执行一条 pending / failed 期次（域公开写入口直调，旧壳
/// `executeScheduledOccurrence` 命令同一执行协议）：经 Writer 接缝归一化折算
/// 落交易行、回填期次、计划完成检查全在域内单正文（事务自持）。返回落地
/// 交易 id。写连接是成对建连的写侧。
pub fn execute_scheduled_occurrence(db: &DbState, occurrence_id: &str) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_scheduled::execute_occurrence(&conn, occurrence_id)
}

/// 预生成下一批期次（域公开写入口直调，旧壳 `expandScheduledOccurrences`
/// 命令同一执行协议）：返回本次新生成的期次 id 列表。写连接是成对建连的
/// 写侧。
pub fn expand_scheduled_occurrences(db: &DbState, id: &str) -> Result<Vec<String>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_scheduled::expand_occurrences(&conn, id)
}

/// 按名即建商户（域公开按名 find-or-create 直调，旧壳 `createMerchant`
/// 命令的重名车间同义——`create_merchant_by_name` 的精确匹配在用名复用 /
/// 未命中即建语义在域内单点，重名陈旧态竞态不再前端逐层兜底）。计划表单
///「输入即建」的自由文本提交路径消费面（旧壳 resolveMerchantRef 的 NSH
/// 等价路径 收口在装配单点）。写连接是成对建连的写侧。
pub fn create_merchant_by_name(db: &DbState, name: &str) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_merchants::create_merchant_by_name(&conn, name)
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
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_savings_goal::create_savings_goal(&conn, input)
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
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_savings_goal::update_savings_goal(&conn, id, input)
}

/// 归档储蓄目标（域入口直调，旧壳 archive_savings_goal 命令同一执行协议）：
/// 退出默认列表，账户 / 流水 / 关联计划原样保留。写连接是成对建连的写侧。
pub fn archive_savings_goal(db: &DbState, id: &str) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_savings_goal::archive_savings_goal(&conn, id)
}

/// 取消归档储蓄目标（域入口直调，旧壳 unarchive_savings_goal 命令同一执行
/// 协议）：恢复 active 回默认列表。写连接是成对建连的写侧。
pub fn unarchive_savings_goal(db: &DbState, id: &str) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_savings_goal::unarchive_savings_goal(&conn, id)
}

/// 删除储蓄目标（域公开写入口直调，旧壳 delete_savings_goal 命令同一执行
/// 协议）：余额非零被域守卫码化拒绝（引导先转出），余额为零时域内同一事务
/// 目标软删 + 级联软删专属账户（历史流水保留可查）。写连接是成对建连的写侧。
pub fn delete_savings_goal(db: &DbState, id: &str) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_savings_goal::delete_savings_goal(&conn, id)
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
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_policy::create_insurer(&conn, input)
}

/// 改名保司（域公开写入口直调，旧壳 `update_insurer` 命令同一执行协议）：
/// 字段省略保持原值、空名拒绝与撞在用同名码化拒绝在域内单正文；不存在
///（或已软删）id → 码化 NotFound。写连接是成对建连的写侧。
pub fn update_insurer(
    db: &DbState,
    id: &str,
    input: ledger_policy::InsurerUpdateInput,
) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_policy::update_insurer(&conn, id, input)
}

/// 软删除保司（域公开写入口直调，旧壳 `delete_insurer` 命令同一执行协议）：
/// 软删不占名字、存量保单引用照常显示（软删不可再被新选择由消费方校验）。
/// 不存在的 id → 码化 NotFound。写连接是成对建连的写侧。
pub fn delete_insurer(db: &DbState, id: &str) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_policy::delete_insurer(&conn, id)
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
