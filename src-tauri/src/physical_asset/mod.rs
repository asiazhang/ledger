//! 实物资产（PhysicalAsset）领域模块（issue #466 / spec #465 / ADR-0064）。
//!
//! 职责：大件实物估值档案的建档（估值必填 = 首条估值历史行）、列表（当前
//! 估值 = 最新一条 + 在持合计）与详情——写入口的校验与归一化、口径接线与
//! 失效信号回调注入。估值机制、与物品域的边界等决策见 ADR-0064 与实物资产
//! 分域词汇表 `docs/contexts/CONTEXT-physical-asset.md`。
//!
//! 接缝：
//! - 域 API（单一权威）：写路径（建档）与读路径（列表/详情），域语言短名；
//!   失效信号以 `notify` 回调注入（保单域同款，先例 `policy` / `item`）。
//! - 金额一律整数分；当前估值折本位币复用 [`crate::transaction::amount`]
//!   接缝（域间横向依赖，ADR-0056 决策 2 允许）。
//!
//! 依赖方向恒为「壳层 → physical_asset → 基础设施」：本模块不反向依赖壳层。

pub mod crud;
mod model;
pub mod validation;

/// 域 API 再导出：调用面用域语言短名（`physical_asset::list_physical_assets` 等），
/// 与 ADR-0056 定格形状一致（先例：`policy` / `item` 入口再导出）。
pub use crud::{create_physical_asset, get_physical_asset, list_physical_assets};
pub use model::{PhysicalAsset, PhysicalAssetInput, PhysicalAssetList, PhysicalAssetStatus};
