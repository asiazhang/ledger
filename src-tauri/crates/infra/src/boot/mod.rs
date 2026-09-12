//! 引导层（ADR-0089 决策 2 的「引导层」落到目录名；ADR-0111 决策 2 / issue
//! #1131）：打开任何库之前必须成立、且与账本数据无关的引导配置与判定，
//! #1131 起自 `db/` 升格为顶层目录模块，本文件只做声明与再导出：
//! - [`disposition`]：启动期库文件处置判定（三态分派）、启动失败门与引导计划
//!   （issue #601 / ADR-0075 决策 5 修订 / ADR-0080；原 `db::boot`，#1131 起改名
//!   让位给引导层目录，避免 boot::boot 同名嵌套）；
//! - [`data_location`]：DataLocation 引导——注册表读取、活动账本目录定位（含
//!   启动期搬迁）与更改意图三步校验（ADR-0018 / ADR-0089）；
//! - [`book_registry`]：账本注册表内核——双格式兼容读取、校验不变量与登记变更
//!   命令（issue #832 / #833 / #836）。账本登记的业务不变量（目录唯一、活动
//!   账本不可移除、首次登记落新格式）留在引导层——必须在无任何数据库连接时
//!   可执行（ADR-0111 已否决迁域，#1131 不重开）；
//! - [`encryption`]：加密引擎基座——文件头探测、整库转换、解锁、忘记口令重置
//!   与锁定门（issue #569–#573 / ADR-0075）；
//! - [`passphrase_cache`]：主口令本机缓存（钥匙串）与生物门形态（issue #574 /
//!   ADR-0075 决策 3）。
//!
//! 依赖方向单向：boot → db（建连 / 迁移 / 完整性检查等库与连接机制经
//! [`crate::db`] 消费），db 的库面模块不引用引导层。既有调用点
//! `crate::db::{boot, data_location, book_registry, encryption,
//! passphrase_cache}::…` 路径经 `db/mod.rs` 再导出保持零改动（#1128 的 ids
//! 同款口径：再导出是路径兼容面，非机制依赖）。测试外挂 `tests/`
//! （`tests.rs` 声明 + 按主题子模块），与 `db/tests/` 同形。

pub mod book_registry;
pub mod data_location;
pub mod disposition;
pub mod encryption;
pub mod passphrase_cache;

#[cfg(test)]
mod tests;
