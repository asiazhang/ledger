// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块）经 crate 根 cfg(test) 整体放行，生产构建
// 零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

//! 实物资产领域 crate（PhysicalAsset，issue #466 / spec #465 / ADR-0064；
//! spec #1086 / issue #1102 自根包域目录拆出，P3 叶子业务域 crate）：大件实物
//! 估值档案的建档（估值必填 = 首条估值历史行）、列表（当前估值 = 最新一条 +
//! 在持合计）、详情、编辑档案（名称 / 购买信息）与更新估值（追加历史行）——
//! 写入口的校验与归一化、口径接线与失效信号回调注入。估值机制、与物品域的
//! 边界等决策见 ADR-0064 与实物资产分域词汇表
//! `docs/contexts/CONTEXT-physical-asset.md`。
//!
//! 接缝：
//! - 域 API（单一权威）：写路径（建档 / 编辑 / 更新估值）与读路径（列表/详情），
//!   域语言短名；失效信号以 `notify` 回调注入（保单域同款，先例 `policy` /
//!   `item`）。
//! - 金额一律整数分；当前估值折本位币复用 [`ledger_transaction::amount`]
//!   接缝（域间横向依赖，ADR-0056 决策 2 允许）。
//!
//! 依赖方向（spec #1086 / issue #1102 AC）：本 crate 消费基础设施、同步协议与
//! 核心交易域（`ledger-infra` / `ledger-sync-protocol` / `ledger-transaction`）
//! ——当前估值折本位币消费交易域 Amount 口径，对根包（壳层）与任何同级业务域
//! 零依赖，反向引用由 cargo 依赖图编译期拒绝（生产依赖面无根包，dev-dependency
//! 环只覆盖测试目标；机器面负向核对住结构守门的 crate 依赖方向，Cargo.toml
//! 注释留痕）。
//!
//! **测试实例纪律（dev-dependency 环双实例，ledger-transaction/#1092 同款）**：
//! `cargo test -p ledger-physical-asset` 的依赖图内存在本 crate 的两份实例——
//! 被测本实例与根包图内实例（静态与类型身份分离）。本域行为路径不读任何接缝
//! 注册静态（失效信号以 `notify` 回调注入，无进程级接缝），域单测直接驱动本
//! 实例；经壳层与跨域口径的旅程由根包侧三层测试（API/命令集成、e2e BDD）走
//! 根包图实例覆盖，断言与场景文本零改动。

mod command;
mod crud;
mod model;
mod validation;

/// 同步重放接缝（跨 crate 消费：sync_engine::registry 经根包再导出面分派外来
/// 实物资产命令，与本地写同协议。#1102 拆 crate 起 `pub(crate)`→`pub`，签名
/// 与语义不变，#1092 replay_command 同款）。
pub use command::replay_command;
pub use command::{PhysicalAssetCommand, ValuationCommandRow};

/// 域 API 再导出：调用面用域语言短名（`physical_asset::list_physical_assets` 等），
/// 与 ADR-0056 定格形状一致（先例：`policy` / `item` 入口再导出）。
pub use crud::{
    create_physical_asset, delete_physical_asset, dispose_physical_asset, get_physical_asset,
    list_physical_assets, update_physical_asset, update_physical_asset_valuation,
};
pub use model::{
    PhysicalAsset, PhysicalAssetDisposeInput, PhysicalAssetInput, PhysicalAssetList,
    PhysicalAssetStatus, PhysicalAssetUpdateInput, PhysicalAssetValuationInput,
};
