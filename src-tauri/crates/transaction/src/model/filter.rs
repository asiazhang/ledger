//! 交易列表查询过滤条件（共享语义区，issue #423 / spec #1025）。
//!
//! 职责：服务端分页 + 各维度过滤条件与 `kinds` 双形态反序列化。不变量：过滤维度与其余
//! 维度 AND 组合、维度内取或；空 kinds 视为未携带。ADR 指针：ADR-0113 决策 8。陷阱：
//! HTTP 查询串为逗号分隔单参数，未知值报 400。

use serde::{Deserialize, Serialize};

use crate::amount::TransactionKind;

/// 交易列表查询过滤条件（服务端分页 + 过滤）。
///
/// 与 `InstrumentListFilter` 先例对齐（`page_size` 下划线命名，serde 保持原样透传）。
/// 分页语义：`page` 从 1 起、缺省 1；`page_size` 缺省时返回全部（`total` 恒返回）；
/// `limit` 为独立的"取前 N 条"参数（仪表盘"最近 N 条"场景），传 `page_size` 时分页路径生效。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionListFilter {
    /// 起始日期（含），YYYY-MM-DD。
    pub from: Option<String>,
    /// 结束日期（含），YYYY-MM-DD。
    pub to: Option<String>,
    /// 按转出账户过滤。
    pub account_id: Option<String>,
    /// 涉及账户过滤（v0.2.0 之后新增扩展字段）：转出 ∪ 转入 ∪ 出资三端——
    /// `account_id = X OR to_account_id = X OR funding_account_id = X`，命中普通交易、
    /// 转账两侧与带出资账户 buy/sell 的出资端（issue #937 / ADR-0096）。
    /// 已发布字段 `account_id`（仅转出账户）语义保持不变，遵守发布冻结约定。
    pub involving_account_id: Option<String>,
    /// 按商户过滤（issue #191）：命中 `merchant_id = X` 的全部未删除交易
    /// （交易行未删即命中，商户本身可已软删——软删商户的历史交易同样可过滤）。
    pub merchant_id: Option<String>,
    /// 按分类精确过滤（issue #377）：命中 `category_id = X` 的未删除交易。
    /// 精确匹配，不含子分类（子分类需自身 id 直查）；分类本身可已软删——
    /// 软删分类的历史交易同样可过滤（历史交易口径，先例商户）。
    pub category_id: Option<String>,
    /// 仅无分类（issue #377）：命中 `category_id IS NULL` 的未删除交易；
    /// `false` 视为未携带（不过滤）。与 `category_id` 同时携带时按 SQL AND 组合
    /// （两条件矛盾，恒为空集）；前端分类维度为单选三态（不过滤/精确/仅无分类），
    /// 不会同时携带两者。
    pub uncategorized_only: Option<bool>,
    /// 按标的过滤（ADR-0107）：命中投资域扩展表 `security_transactions` 中该标的的行——
    /// buy/sell/dividend/split 由 `instrument_id`（转出腿）命中，convert **任一腿命中即算**
    /// （转入腿 `to_instrument_id`；与时点持仓推算认 convert 两腿的既有口径对齐）。
    /// 经子查询实现（核心交易行不持标的信息），与其余维度 AND 组合。
    pub instrument_id: Option<String>,
    /// 交易类型集合过滤（spec #1025 起为唯一类型维度，手动多选与下钻载荷共用）：
    /// 命中 `kind IN (...)` 的未删除交易，维度内取或、与其余维度 AND 组合。
    /// 单值亦经本参数传递——原单值 `kind` 查询参数已移除（BREAKING，未发布窗口内
    /// 就地变更，见 CHANGELOG）：旧调用方传 `kind=` 不报错、被忽略、结果变宽。
    /// 两形态反序列化（[`deserialize_kind_set`]）：IPC JSON 为字符串数组；HTTP 查询串
    /// 为逗号分隔单参数（`kinds=expense,refund`，与前端下钻 URL 同一编码），逐元素
    /// 闭集枚举、未知值报参数错误（400）。空数组视为未携带（不过滤，先例同
    /// `uncategorized_only=false`）；HTTP 空串（`kinds=`）不是合法字面量串，照报 400
    /// （先例同 `uncategorized_only=`）。
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_kind_set"
    )]
    pub kinds: Option<Vec<TransactionKind>>,
    /// 取前 N 条（仪表盘"最近 N 条"场景），与分页互斥：传 `page_size` 时分页路径生效。
    /// 沿用 SQLite 原生语义：`limit=0` 返回空，负值无上限。
    pub limit: Option<i64>,
    /// 页码，从 1 开始，默认 1。
    pub page: Option<usize>,
    /// 每页条数，缺省返回全部（total 恒返回）；小于 1 按 1 处理。
    pub page_size: Option<usize>,
}

/// `kinds` 字段双形态反序列化（issue #581，spec #1025 起兼任单值载体）：字符串数组
/// （IPC JSON）或逗号分隔单参数（HTTP 查询串，与前端下钻 URL 同一编码）。逐元素经
/// [`TransactionKind`] 闭集枚举反序列化，未知值报参数错误（400）。
fn deserialize_kind_set<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<TransactionKind>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct KindSetVisitor;

    impl<'de> serde::de::Visitor<'de> for KindSetVisitor {
        type Value = Option<Vec<TransactionKind>>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("kind 字符串数组或逗号分隔字符串")
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
            let mut kinds = Vec::new();
            for s in v.split(',') {
                match TransactionKind::parse(s.trim()) {
                    Ok(k) => kinds.push(k),
                    Err(e) => return Err(serde::de::Error::custom(e.to_string())),
                }
            }
            Ok(Some(kinds))
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut kinds = Vec::new();
            while let Some(k) = seq.next_element::<TransactionKind>()? {
                kinds.push(k);
            }
            Ok(Some(kinds))
        }

        fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }
    }

    deserializer.deserialize_any(KindSetVisitor)
}
