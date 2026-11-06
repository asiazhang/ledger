//! 域栈引导（issue #2017 / ADR-0145）：打开未加密账本库，读交易列表页与币种
//! 字典（issue #2018 交易表格首屏的读面）。
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

use ledger_categories::Category;
use ledger_currencies::Currency;
use ledger_infra::db::boot::{BOOT_DB_UNREADABLE, BootDisposition};
use ledger_infra::db::data_location::DB_FILE_NAME;
use ledger_infra::db::{DbState, open_db_in};
use ledger_infra::error::{AppError, Result};
use ledger_transaction::TransactionListResult;

use crate::assembly::PAGE_SIZE;
use crate::wiring;

/// 密文库不支持（立桩范围外）的稳定错误码。
pub const CODE_ENCRYPTED_UNSUPPORTED: &str = "nsh-shell.encrypted-unsupported";

/// 账本库文件缺失的稳定错误码。
pub const CODE_BOOK_MISSING: &str = "nsh-shell.book-missing";

/// 外来形态归一化失败的稳定错误码（与旧壳 `boot.db-normalize-failed` 同码，
/// 基础设施 [`ledger_infra::db::boot::BOOT_DB_NORMALIZE_FAILED`]）。
const BOOT_DB_NORMALIZE_FAILED: &str = ledger_infra::db::boot::BOOT_DB_NORMALIZE_FAILED;

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

/// 读取账本库交易列表页（读路径直调域服务，issue #2018 立桩靶标；issue #2021
/// 补日期区间维度）。
///
/// 经核心交易域读路径 [`ledger_transaction::list_transactions`] 的服务端 offset
/// 分页（ADR-0008 语义：页码从 1 起、`total` 与行集共用同一 WHERE 子句、排序
/// date DESC, created_at DESC, id DESC），每页 [`PAGE_SIZE`] 行（100 行/页，
/// 旧壳 50 行翻倍验证原生余量）；`range` 为期间快照（含边界，自然周期换算产物
/// 见 [`crate::period`]），None = 「全部」无过滤（立桩搜索随 #2023 接入）。
/// 来源列等读时投影接缝已由 [`crate::wiring`] 接线。读连接是成对建连的只读侧
/// （ADR-0117：读路径不进写者闸门）。
pub fn transaction_page(
    db: &DbState,
    page: u32,
    range: Option<&crate::period::PeriodRange>,
) -> Result<TransactionListResult> {
    let filter = ledger_transaction::TransactionListFilter {
        page: Some(page.max(1) as usize),
        page_size: Some(PAGE_SIZE),
        from: range.map(|r| r.from.clone()),
        to: range.map(|r| r.to.clone()),
        ..Default::default()
    };
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| AppError::Db(e.to_string()))?;
    ledger_transaction::list_transactions(&conn, &filter)
}

/// 读取账本库数据期间边界（issue #2021 / ADR-0141 期间步进的钳制输入）：
/// 报表域日期极值读路径 [`ledger_reports::query_report_date_range`]——旧壳
/// report_date_range 消费的同一域函数；双 None = 空库。随每页读一并刷新
/// （单行聚合查询，成本可忽略；写路径落地后天然跟随数据外扩/收窄，
/// 无需独立失效接线）。
pub fn report_date_range(db: &DbState) -> Result<ledger_reports::DateRange> {
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| AppError::Db(e.to_string()))?;
    ledger_reports::query_report_date_range(&conn)
}

/// 读取账本库币种字典（金额展示的参考数据面，issue #2018）：装配层的币种
/// 展示参数（符号/小数位）来自本只读投影；随每页读一并刷新（字典行数小，
/// 改名/改号后的重读成本可忽略）。
pub fn currency_rows(db: &DbState) -> Result<Vec<Currency>> {
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| AppError::Db(e.to_string()))?;
    ledger_currencies::list_currencies(&conn)
}

/// 读取在用分类行集（分类路径解析的参考数据面，issue #2019）：装配层的分类
/// 路径表来自本只读投影，**仅未删除**——旧壳路径函数消费在用分类行集
/// （reference store 口径），软删分类的历史交易引用在交易列表显示 `-`；随每页
/// 读一并刷新（分类行数小，改名/改父后的重读成本可忽略）。
pub fn category_rows(db: &DbState) -> Result<Vec<Category>> {
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| AppError::Db(e.to_string()))?;
    ledger_categories::list_categories(&conn, false)
}
