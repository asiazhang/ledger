//! `write` 模块测试索引（ADR-0113 决策 6：测试随生产模块外挂）。
//!
//! - [`balance_cache`]：余额/净资产持久化缓存一致性（issue #491 / ADR-0067）——
//!   覆盖全部写入入口（创建/修改/删除/批量/Writer 直写/余额调整/账户增删）

mod balance_cache;
