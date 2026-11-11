//! 域栈引导（issue #2017 / ADR-0145）：打开未加密账本库，建立域栈会话，并作为
//! 壳侧域读写接缝的总入口。
//!
//! **结构（#2073 分域拆分）**：引导内核住本文件（处置判定 / 成对建连 / 启动编排 /
//! 选目录处置 / 引导记录槽）；域接缝按域分文件（各文件头自述域来源票号与语义），
//! 本文件 `pub use` 全量再导出——消费面 `boot::函数名` 不变。分域住址：
//! `transactions`（交易页/搜索/删除）、`items`、`backup`（最小入口）、`txn_form`
//! （表单族参考数据与基础收支写，高扇入）、`budget`、`dashboard`（概览）、
//! `reports`、`investment`、`accounts`、`scheduled`、`merchants`、`savings`、
//! `policies`、`insurers`、`physical_assets`、`settings`（日志档位 /
//! DataLocation）、`data_safety`（备份管理 / 加密 / 多端同步 / 账本注册表）。
//! 新增域接缝落在对应域文件，不再追加进内核。
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

use ledger_infra::db::boot::{BOOT_DB_UNREADABLE, BootDisposition};
use ledger_infra::db::data_location::DB_FILE_NAME;
use ledger_infra::db::{DbState, open_db_in};
use ledger_infra::error::{AppError, Result};

use crate::wiring;

mod accounts;
mod backup;
mod budget;
mod dashboard;
mod data_safety;
mod insurers;
mod investment;
mod items;
mod merchants;
mod physical_assets;
mod policies;
mod reports;
mod savings;
mod scheduled;
mod settings;
mod transactions;
mod txn_form;

pub use accounts::*;
pub use backup::*;
pub use budget::*;
pub use dashboard::*;
pub use data_safety::*;
pub use insurers::*;
pub use investment::*;
pub use items::*;
pub use merchants::*;
pub use physical_assets::*;
pub use policies::*;
pub use reports::*;
pub use savings::*;
pub use scheduled::*;
pub use settings::*;
pub use transactions::*;
pub use txn_form::*;

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
/// 由调用方在缺参之前分流，不经本函数）。返回 `Ok(None)` = 注册表未配置
///（含损坏回退）且解析落点无库——首启引导现场，呈现面据此呈现引导态
///（ADR-0146 决策 2）；现场分流以 [`ledger_infra::db::data_location::RegistryBootState`]
/// 状态信号判定（未配置/损坏回退 vs 已解析活动条目），不以路径比较判定。
/// 注册表已解析活动条目时库文件缺失——无论活动本指向哪个目录——一律
/// book-missing fail loud（决策 4；失败态自带「选择账本库目录…」出口）。
/// 引导现场的引导计划随返回落槽（含回退原因 `fallback_reason`），呈现面经
/// [`boot_record`] 消费（引导态损坏告示与现场 D 的就绪回退告示同源）。
pub fn boot_ledger_via_registry(default_dir: &Path) -> Result<Option<DbState>> {
    wiring::install_domain_seams();
    let plan = ledger_infra::db::boot::plan_boot(default_dir);
    let db_path = plan.boot.db_dir.join(DB_FILE_NAME);
    if !db_path.is_file() {
        use ledger_infra::db::data_location::RegistryBootState;
        match plan.boot.registry_state {
            // 未配置 / 损坏回退且落点无库：首启引导现场（ADR-0146 决策 2），
            // 不 fail loud 也不静默新建；引导计划落槽（回退原因随槽，呈现面
            // 经 boot_record 消费——与就绪分支的回退告示同源同款式）。
            RegistryBootState::Unconfigured | RegistryBootState::Corrupt => {
                *boot_record_slot() = Some(plan.boot);
                return Ok(None);
            }
            // 已解析活动条目缺库：fail loud（决策 4；含活动本指向默认目录
            // 自身的现场——路径等值不能区分「未配置」与「已配置」）。
            RegistryBootState::Resolved => return Err(book_missing_error(&db_path)),
        }
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
/// 复用面（T4 分叉后的现实，F8 改述）：新建两腿各自登记策略、共享建库
/// 单点（[`provision_book_db`]）——本腿（首启引导「新建账本」）登记折叠为
/// 「默认账本」直写 `write_registry`；选择器新建腿（[`confirm_create_picked_book`]，
/// ADR-0146 决策 3 ③）登记经 `register_or_switch_book_dir` 铸条目置活动
///（损坏容错折叠与本腿同向）。会话开启不在本函数内——调用方经各自解析链
/// 臂开启（首启确认后重走启动竖井，本次启动即进主界面）。
pub fn create_default_book(dir: &Path) -> Result<()> {
    wiring::install_domain_seams();
    provision_book_db(dir)?;
    ledger_infra::db::book_registry::write_registry(
        dir,
        &ledger_infra::db::book_registry::BookRegistry::single_default(dir),
    )
}

/// 建库单点（新建两腿共享，F8 收口）：确保目录存在后经建连迁移单点
///（[`open_db_in`]，缺文件建连即建全新库）建出库文件；登记策略归调用方
///（两腿各自不同，见 [`create_default_book`] doc 复用面）。
fn provision_book_db(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    open_db_in(dir)?;
    Ok(())
}

/// 库文件缺失的码化错误（fail loud 单点；argv 打错与注册表落点无库两臂
/// 共用同码同文案）。文案随立桩收窄解除重写为正式呈现（ADR-0146 决策 4）；
/// #2015 防假空态内核保留——不自动新建的立场写进文案，出口交给失败态的
/// 「选择账本库目录…」按钮。
fn book_missing_error(db_path: &Path) -> AppError {
    AppError::codedp(
        CODE_BOOK_MISSING,
        format!(
            "账本库文件不存在：{}。应用不会自动新建账本库（防止误指向空目录造成空账假象）；请确认目录后重试，或选择其他账本库目录。",
            db_path.display()
        ),
        &[&db_path.to_string_lossy()],
    )
}

/// 密文库不支持的码化错误（文案单点：启动引导与目录选择闭集分支④共用，
/// 解锁向导归 #2088）。
fn encrypted_unsupported_error() -> AppError {
    AppError::coded(
        CODE_ENCRYPTED_UNSUPPORTED,
        "密文库暂不支持：当前版本仅支持打开未加密账本库，解锁功能将在后续版本提供",
    )
}

/// 损坏残留的码化错误（文案单点：与启动引导同码同文案，目录选择闭集分支⑤
/// 复用；不自动换目录重试的语义随呈现面表达）。
fn db_unreadable_error() -> AppError {
    AppError::coded(BOOT_DB_UNREADABLE, "数据库文件无法打开（文件可能已损坏）")
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
        BootDisposition::AwaitUnlock => Err(encrypted_unsupported_error()),
        BootDisposition::Unreadable => Err(db_unreadable_error()),
    }
}

/// 选中目录的现场闭集处置（ADR-0146 决策 3 单点）：book-missing 失败态的
/// 「选择账本库目录…」按钮与首启引导「打开已有账本」（T3）两个消费面共用
/// 同一函数——现场分流语义只有这一份。
///
/// 分流闭集（判定序即闭集序）：
/// ① 注册表在册（含活动本自身，目录身份物理折叠）→ 切活动指针（已是活动
///    本幂等零写入）→ [`PickedBookDisposition::Open`]；
/// ②③ 未在册：无库文件 → [`PickedBookDisposition::ConfirmCreate`]（新建
///    意图，调用方呈现确认对话明示完整路径后经 [`confirm_create_picked_book`]
///    完成）；有库 → 登记 + 置活动 → `Open`；
/// ④⑤ 库文件为密文 / 损坏残留 → 既有码化错误原样上抛，注册表零写入，
///    不自动换目录重试（在册与否同一判定序，F3 先分类后切指针）。
///
/// 损坏注册表按「未注册」容错（ADR-0146 决策 3）：查册失败且原因是损坏
///（[`ledger_infra::db::book_registry::REGISTRY_CORRUPT`]）→ 视为未在册走
/// 文件分类路径，登记写入覆盖损坏文件（与首启新建腿「告示知情后覆盖损坏
/// 指针」裁决同向，[`register_or_switch_book_dir`] 同款容错）——损坏告示
/// 现场的选择器不再停在「请先在数据设置中恢复默认位置」的不可达死路。
/// 密文库与损坏**库文件**的分类错误（④⑤）不受影响，原样上抛。
///
/// 返回 `Open` 后的「打开」归调用方（失败态按钮按启动臂重跑启动竖井，引导
/// 动作按引导现场打开），本函数只保证注册表已就位。
#[derive(Debug, PartialEq, Eq)]
pub enum PickedBookDisposition {
    /// 注册表已就位（切指针 / 登记+置活动 / 已是活动本幂等），调用方打开。
    Open,
    /// 目录无库：按新建意图处理，待确认对话后经 [`confirm_create_picked_book`]
    /// 完成（携带明示路径用的目录）。
    ConfirmCreate(PathBuf),
}

pub fn resolve_picked_book_dir(
    registry_root: &Path,
    picked_dir: &Path,
) -> Result<PickedBookDisposition> {
    use ledger_infra::db::book_registry::{register_or_switch_book_dir, registered_book_at_dir};
    // 先分类后切指针（F3 / ADR-0146 决策 3④⑤ + 决策 4）：库文件在场即先做
    // 处置判定（与启动引导同源 classify_for_boot，不另立第二份判定；在册与
    // 否同一判定序）——密文/损坏码化错误上抛、注册表零写入；选中在册密文/
    // 损坏本不再先切指针再失败（指针已移走 + 该失败态无选择器 = 应用内
    // 不可恢复）。明文/归一化继续在册/登记分流，分类通过的打开行为不变。
    let db_path = picked_dir.join(DB_FILE_NAME);
    if db_path.is_file() {
        match ledger_infra::db::boot::classify_for_boot(&db_path)? {
            BootDisposition::AwaitUnlock => return Err(encrypted_unsupported_error()),
            BootDisposition::Unreadable => return Err(db_unreadable_error()),
            BootDisposition::OpenPlaintext | BootDisposition::NormalizePlaintext => {}
        }
    }
    let registered = match registered_book_at_dir(registry_root, picked_dir) {
        Ok(found) => found,
        // 损坏注册表按「未注册」容错（ADR-0146 决策 3，见函数文档）；其他
        // 查册失败原样上抛。
        Err(e) if e.is_code(ledger_infra::db::book_registry::REGISTRY_CORRUPT) => None,
        Err(e) => return Err(e),
    };
    if registered.is_none() {
        // 未在册：无库文件 → 新建意图（调用方呈现确认对话明示完整路径后经
        // [`confirm_create_picked_book`] 完成）。
        if !db_path.is_file() {
            return Ok(PickedBookDisposition::ConfirmCreate(
                picked_dir.to_path_buf(),
            ));
        }
    }
    register_or_switch_book_dir(registry_root, picked_dir)?;
    Ok(PickedBookDisposition::Open)
}

/// 确认新建后的完成动作（决策 3 分支③）：先建库（[`open_db_in`] 建连迁移
/// 单点，缺文件即新装语义；目录不存在先建目录）后登记（未配置根折叠为
/// 「默认账本」条目，否则铸新条目置活动，ADR-0146 决策 5）。建库失败 →
/// 注册表零写入：确认对话可重试或换目录重选，不留「已登记但无库」的悬死
/// 现场（旧序「登记先行」的失败重试宣称失实，F4 换序后宣称成立）。
pub fn confirm_create_picked_book(registry_root: &Path, picked_dir: &Path) -> Result<()> {
    provision_book_db(picked_dir)?;
    ledger_infra::db::book_registry::register_or_switch_book_dir(registry_root, picked_dir)?;
    Ok(())
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

