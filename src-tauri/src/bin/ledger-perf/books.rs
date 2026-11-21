use std::path::{Path, PathBuf};

use ledger_infra::db::book_registry::Book;
use ledger_infra::db::{open_connection_in, open_connection_readonly_in, schema_version};

use super::generate::{GenerateParams, generate_into};

pub(crate) const BOOKS_DIR_NAME: &str = "ledger-perf-books";
pub(crate) const ATTACHED_BOOK_TOTAL: usize = 2;
pub(crate) const ATTACHED_BOOK_TRANSACTIONS: u64 = 2_000;

pub(crate) fn attached_books_root(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .map(|p| p.join(BOOKS_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from(BOOKS_DIR_NAME))
}

pub(crate) fn attached_book_dir_name(i: usize) -> String {
    format!("book-{i:02}")
}

pub(crate) fn attached_book_db_path(books_root: &Path, i: usize) -> PathBuf {
    books_root
        .join(attached_book_dir_name(i))
        .join(ledger_infra::db::data_location::DB_FILE_NAME)
}

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
