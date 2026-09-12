//! 交易列表标的来源反查（spec #704 / issue #709，词汇表「来源列」）：按生成
//! 交易 id 批量取证券交易记录指向的标的展示字段，供核心交易域按页填充来源列
//! （按页一次 IN 查询，不做逐行 N+1）。
//!
//! 边界（词汇表「来源列」标的分支）：
//! - 证券交易记录（`security_transactions`）的 `transaction_id` 是主键——
//!   一笔交易至多一行，来源反查天然无多义；
//! - 展示层反查、零新增指针：来源是读时推导，不落任何数据级反向引用；
//! - 标的字典无软删（被流水引用的标的不可删，[`crud::delete_instrument`] 守卫
//!   与外键 RESTRICT 双保险），反查恒命中、无状态标注——清仓标的的历史交易
//!   同样可达（走势不依赖持仓，实体定位参数词条「落点形态按视图」）。

use rusqlite::Connection;

use super::model::InstrumentSourceDisplay;
use crate::db::query::query_all;
use crate::error::Result;

/// 按生成交易 id 批量反查买卖明细指向的标的（标的 id + 代码 + 名称），调用方
/// 按 `transaction_id` 把结果归位到交易行。split 已随 ADR-0106 / #1049、
/// dividend 已随 ADR-0109 / #1078 激活，两者经同一路径自然获得标的来源；本查询
/// 不按 action 过滤——反查语义是「交易发起源档案」，不是买卖动作本身。
pub fn source_display_by_transaction_ids(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<Vec<InstrumentSourceDisplay>> {
    if transaction_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; transaction_ids.len()].join(",");
    query_all(
        conn,
        &format!(
            "SELECT st.transaction_id, st.instrument_id, i.symbol, i.name \
             FROM security_transactions st JOIN instruments i ON i.id = st.instrument_id \
             WHERE st.transaction_id IN ({placeholders})",
        ),
        rusqlite::params_from_iter(transaction_ids.iter()),
    )
}
