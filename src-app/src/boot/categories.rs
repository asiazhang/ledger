//! 分类管理读写接缝（#2136 设置③）：分类写路径（创建 / 编辑 / 软删 /
//! 排序重排）直调 `ledger-categories` 域入口——与旧壳 settings.categories 消费
//! 的 createCategory / updateCategory / deleteCategory / reorderCategories 命令
//! 同一执行协议，不经 IPC。两级分类校验、预算删除守卫、排序重排全集覆盖语义
//! 全在域内单正文，本壳不自建守卫语义；清单读复用 [`super::txn_form`] 的
//! `category_rows` 只读投影不另立读接缝。

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use crate::shell_support::write_entry_locked;

/// 创建分类（域公开写入口直调）：两级分类校验（父分类须存在、kind 一致）在
/// 域内单点，码化错误原样上抛呈现。写连接是成对建连的写侧。
pub fn create_category(db: &DbState, input: ledger_categories::CategoryInput) -> Result<String> {
    write_entry_locked(db, |conn| ledger_categories::create_category(conn, input))
}

/// 编辑分类（域公开写入口直调）：名称 / 图标 / 父分类三态可选字段（未传保持
/// 原值），两级分类校验同上。写连接是成对建连的写侧。
pub fn update_category(
    db: &DbState,
    id: &str,
    input: ledger_categories::CategoryUpdateInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_categories::update_category(conn, id, input)
    })
}

/// 软删分类（域公开写入口直调）：预算删除守卫（名下有未删除预算时码化拒绝）
/// 在域内单点；历史交易引用照常显示。写连接是成对建连的写侧。
pub fn delete_category(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_categories::delete_category(conn, id))
}

/// 同层排序重排（域公开写入口直调）：全集覆盖语义（提交顺序 = 落定序），
/// 同一父分类下的兄弟序列整批落 `sort_order`。写连接是成对建连的写侧。
pub fn reorder_categories(db: &DbState, items: Vec<ledger_categories::ReorderItem>) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_categories::reorder_categories(conn, items)
    })
}
