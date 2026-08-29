//! 投资域命令测试索引（issue #257：原单文件 tests.rs 按行为主题拆分为目录子模块，
//! 纯移动——断言、夹具与 mock 语义不变，测试不增删不改名）。
//!
//! - [`common`]：建库 / 夹具插入 / 交易输入构造等共享脚手架
//! - [`instrument_list`]：标的列表、搜索、invested 派生、CRUD 与持仓视图
//! - [`predicates`]：「持仓标的」判定谓词 ↔ v_holdings 视图一致性绑定
//! - [`trade`]：buy/sell 写入与买卖明细查询（命名对齐源码 trade 模块）
//! - [`pnl`]：已实现盈亏汇总
//! - [`trend`]：走势查询（单标的 / 组合）
//! - [`holdings_as_of`]：时点持仓推算

mod common;
mod holdings_as_of;
mod instrument_list;
mod pnl;
mod predicates;
mod trade;
mod trend;

// instrument_list 子模块的断言沿用原调用路径 `super::crud::…`：在此把投资域
// crud 模块引入 tests 命名空间，供子模块以原路径解析（纯移动保形）。
use super::crud;
