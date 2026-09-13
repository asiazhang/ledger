// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块与外挂 tests.rs）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。
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

//! 商户领域 crate（Merchant，issue #188 / ADR-0028；spec #1086 / issue #1096 自根包
//! 域目录拆出，参考数据三域各自独立 crate、不合并）：商户字典的 CRUD 与按名
//! 查找/即建均在本 crate 收口；IPC 参数解包、事务边界、命令注册和失效信号发射
//! 留在商户命令壳层。crate 不依赖壳层，商户实体与入参集中 [`model`]（#419 随域
//! 归位），消费方经域路径逐类型显式 import。
//!
//! 交易×商户接缝（spec #1086 / issue #1092）：交易行为层的商户名归一化
//!（先查名、校验通过后即建）经 `ledger-transaction::merchant_seam` 注册点消费，
//! 实现由本 crate [`crud::install_merchant_hooks`] 原子装入（壳层启动接线，
//! ADR-0112 决策 5「下层定义注册点、上层注册实现、壳层启动时接线」）；未注册即
//! 码化错误（失败可见，不静默丢即建）。
//!
//! 依赖方向（spec #1086 / issue #1096 AC）：本 crate 消费基础设施、同步协议与
//! 核心交易域（`ledger-infra` / `ledger-sync-protocol` / `ledger-transaction`），
//! 对根包（壳层）与同步域零直接依赖——同步命令契约、op 产出与设备标识走协议
//! crate，重放分派住 sync_engine（业务域→同步域零容忍，ADR-0101 决策 4b）。
//! 依赖方向由编译期强制（issue #1096 AC3）：生产依赖面（lib 目标）没有根包
//! `tauri_app_lib`，构造对它的引用即编译失败；dev-dependency 环只覆盖测试目标，
//! 不构成生产环（本 crate Cargo.toml 注释留痕）。
//!
//! **测试实例纪律（dev-dependency 环双实例，ledger-transaction/#1092 同款）**：
//! `cargo test -p ledger-merchants` 的依赖图内存在本 crate 的两份实例——被测本
//! 实例与根包图内实例（静态与类型身份分离）。本域行为路径不读交易域接缝注册
//! 静态（接缝方向是交易 → 商户，本 crate 是实现注册侧），域单测直接驱动本实例；
//! 经接缝与壳层的旅程由根包侧三层测试（API/命令集成、e2e BDD）走根包图实例
//! 覆盖，断言与场景文本零改动。

mod command;
mod crud;
mod model;

pub use command::MerchantCommand;
/// 同步重放（跨 crate 消费：sync_engine 分派接缝经根包再导出面执行外来命令，
/// ADR-0091。#1096 拆 crate 起 `pub(crate)`→`pub`，签名与语义不变）。
pub use command::replay_command;
pub use crud::{
    create_merchant, create_merchant_by_name, delete_merchant, find_merchant_by_name, get_merchant,
    install_merchant_hooks, list_merchants, transaction_counts, update_merchant,
};
pub use model::{Merchant, MerchantInput, MerchantTransactionCount, MerchantUpdateInput};

#[cfg(test)]
mod tests;
