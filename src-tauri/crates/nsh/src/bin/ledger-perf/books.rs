//! 附属账本夹具（issue #1630）：generate 的「1 主库 + N 附属账本」多账本
//! 数据集形状。
//!
//! 跨账本投资汇总（CrossBookInvestmentSummary，ADR-0114 / issue #1196）是
//! 多库只读聚合——基准它需要多本真实形状的库，单库数据集形状缺口。附属
//! 账本 = 汇总语境里的非活动本夹具：固定小规模（不随 `--transactions`
//! 缩放）、与主库同构的多域画像（经 [`generate_into`] 全套生成，含标的
//! 交易与批次持仓），每本一个子目录、内含固定名 `ledger.db`——与产品
//! 「一库 = 一本账」的目录形态一致（ADR-0089）；根目录在主库文件同级
//!（[`BOOKS_DIR_NAME`]），generate 每次先清后建（幂等重建，与主库文件
//! 同款纪律）。
//!
//! 确定性：每本由基础种子派生自己的种子（seed + 1 + 序号），主库种子不
//! 受影响（seed 确定性不变）；同参数两次生成逐本一致（无墙钟、无
//! HashMap 遍历纪律与主库同款）。
//!
//! 消费面：bench 的跨账本投资汇总基准项经 [`discover_attached_books`]
//! 前置探测夹具——本数齐、每本已建库、schema 版本与主库一致，任一不满足
//! 即 Err（删除生成侧 → 本探测失败 → 基准运行红，删除即变红）。

use std::path::{Path, PathBuf};

use ledger_infra::db::book_registry::Book;
use ledger_infra::db::{open_connection_in, open_connection_readonly_in, schema_version};

use super::generate::{GenerateParams, generate_into};

/// 附属账本根目录名（主库文件同级；构建目标目录下天然被版本控制忽略）。
pub(crate) const BOOKS_DIR_NAME: &str = "ledger-perf-books";
/// 附属账本本数（「1 主库 + N 附属账本」的 N；变更须同步 tests 与 bin 头注释）。
pub(crate) const ATTACHED_BOOK_TOTAL: usize = 2;
/// 每本附属账本的交易笔数（固定小规模；含标的交易与持仓的最小完整画像）。
pub(crate) const ATTACHED_BOOK_TRANSACTIONS: u64 = 2_000;

/// 附属账本根目录：主库文件同级的固定名目录（`--out` 决定，随主库走）。
pub(crate) fn attached_books_root(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .map(|p| p.join(BOOKS_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from(BOOKS_DIR_NAME))
}

/// 第 i 本附属账本的目录名（book-01 起，确定性命名）。
pub(crate) fn attached_book_dir_name(i: usize) -> String {
    format!("book-{i:02}")
}

/// 第 i 本附属账本的库文件路径（root/book-NN/ledger.db）：生成、探测与测试
/// 共用的拼装单点，目录形状改动不散落多处。
pub(crate) fn attached_book_db_path(books_root: &Path, i: usize) -> PathBuf {
    books_root
        .join(attached_book_dir_name(i))
        .join(ledger_infra::db::data_location::DB_FILE_NAME)
}

/// 生成附属账本（generate 末段调用）：根目录先清后建，逐本独立走产品迁移
/// 建库（[`open_connection_in`]）+ 全套画像生成（[`generate_into`]，余额
/// 缓存回填与 ANALYZE 随每本完成）；返回各本目录（序即种子派生序）。
pub(crate) fn generate_attached_books(
    db_path: &Path,
    base: &GenerateParams,
) -> Result<Vec<PathBuf>, String> {
    let root = attached_books_root(db_path);
    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|e| format!("清理旧附属账本目录失败：{e}"))?;
    }
    std::fs::create_dir_all(&root).map_err(|e| format!("创建附属账本目录失败：{e}"))?;
    let mut dirs = Vec::with_capacity(ATTACHED_BOOK_TOTAL);
    for i in 0..ATTACHED_BOOK_TOTAL {
        let dir = root.join(attached_book_dir_name(i));
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建附属账本目录失败：{e}"))?;
        let mut conn = open_connection_in(&dir).map_err(|e| format!("附属账本建库失败：{e}"))?;
        let params = GenerateParams {
            seed: base.seed + 1 + i as u64,
            transactions: ATTACHED_BOOK_TRANSACTIONS,
            end_date: base.end_date,
        };
        generate_into(&mut conn, &params)
            .map_err(|e| format!("附属账本生成失败（{}）：{e}", dir.display()))?;
        dirs.push(dir);
    }
    Ok(dirs)
}

/// 探测附属账本夹具（bench 前置探测，不计入任何基准）：本数齐、每本已建库、
/// schema 版本与主库一致（版本判据与生产 collect_other_books 同款——不一致
/// 即被排除，基准必须跑在夹具完整形态上而非静默少算）。任一不满足即 Err，
/// 指向 ledger-perf generate 重生成。参数＝附属账本根目录
///（[`attached_books_root`] 推导单点），不做二次推导。
pub(crate) fn discover_attached_books(
    books_root: &Path,
    active_schema_version: i64,
) -> Result<Vec<Book>, String> {
    let mut books = Vec::with_capacity(ATTACHED_BOOK_TOTAL);
    for i in 0..ATTACHED_BOOK_TOTAL {
        let dir = books_root.join(attached_book_dir_name(i));
        let missing = || {
            format!(
                "附属账本夹具缺失（{}）——跨账本投资汇总基准需要 1 主库 + \
                 {ATTACHED_BOOK_TOTAL} 附属账本，先运行 ledger-perf generate",
                dir.display()
            )
        };
        if !dir.is_dir() || !attached_book_db_path(books_root, i).is_file() {
            return Err(missing());
        }
        let conn = open_connection_readonly_in(&dir)
            .map_err(|e| format!("附属账本只读打开失败（{}）：{e}", dir.display()))?;
        let version = schema_version(&conn)
            .map_err(|e| format!("附属账本 schema 版本读取失败（{}）：{e}", dir.display()))?;
        if version != active_schema_version {
            return Err(format!(
                "附属账本 schema 版本与主库不一致（{}：{version} ≠ \
                 {active_schema_version}）——先运行 ledger-perf generate 重新生成",
                dir.display()
            ));
        }
        books.push(Book {
            id: attached_book_dir_name(i),
            name: attached_book_dir_name(i),
            dir,
        });
    }
    Ok(books)
}
