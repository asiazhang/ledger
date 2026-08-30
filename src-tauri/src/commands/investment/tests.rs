//! 投资域命令测试索引（issue #257：原单文件 tests.rs 按行为主题拆分为目录子模块，
//! 纯移动——断言、夹具与 mock 语义不变，测试不增删不改名）。
//!
//! - [`common`]：建库 / 夹具插入 / 交易输入构造等共享脚手架
//! - [`instrument_list`]：标的列表、搜索、invested 派生、CRUD 与持仓视图
//! - [`instrument_create`]：标的创建的来源标记（同步 / 手动）与复用不覆盖
//! - [`instrument_manual_create`]：手动创建入口守卫（类型白名单 + 名称必填，issue #290）
//! - [`fund_add`]：按代码即拉添加基金（注入 stub，issue #301 / ADR-0038）
//! - [`predicates`]：「持仓标的」判定谓词 ↔ v_holdings 视图一致性绑定
//! - [`trade`]：buy/sell 写入与买卖明细查询（命名对齐源码 trade 模块）
//! - [`pnl`]：已实现盈亏汇总
//! - [`trend`]：走势查询（单标的 / 组合）
//! - [`holdings_as_of`]：时点持仓推算

mod common;
mod fund_add;
mod holdings_as_of;
mod instrument_create;
mod instrument_list;
mod instrument_manual_create;
mod pnl;
mod predicates;
mod trade;
mod trend;

// instrument_list 子模块的断言沿用原调用路径 `super::crud::…`：在此把投资域
// crud 模块引入 tests 命名空间，供子模块以原路径解析（纯移动保形）。
use super::crud;
