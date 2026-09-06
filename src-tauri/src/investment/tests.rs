//! 投资域测试索引（issue #257：原单文件 tests.rs 按行为主题拆分为目录子模块，
//! 纯移动——断言、夹具与 mock 语义不变，测试不增删不改名；#401 域归位随迁）。
//!
//! - [`common`]：建库 / 夹具插入 / 交易输入构造等共享脚手架
//! - [`instrument_list`]：标的列表、搜索、invested 派生、CRUD 与持仓视图
//! - [`instrument_create`]：标的创建的来源标记（同步 / 手动）与复用不覆盖
//! - [`instrument_manual_create`]：手动创建入口守卫（类型白名单 + 名称必填，issue #290）
//! - [`instrument_delete`]：自建标的删除守卫两态（issue #292 / ADR-0036 决策 5）
//! - [`fund_add`]：按代码即拉添加基金（注入 stub，issue #301 / ADR-0038）
//! - [`manual_price`]：手动报价两落点与信号发射判定（issue #291 / ADR-0036）
//! - [`predicates`]：「持仓标的」判定谓词 ↔ v_holdings 视图一致性绑定
//! - [`trade`]：buy/sell 写入与买卖明细查询（命名对齐源码 trade 模块）
//! - [`fund_trade`]：场外基金申赎记账——金额权威、单价反算、成本锚定与盈亏闭合不变式（issue #302 / ADR-0038）
//! - [`pnl`]：已实现盈亏汇总
//! - [`trend`]：走势查询（单标的 / 组合）
//! - [`holdings_as_of`]：时点持仓推算
//! - [`stock_lookup`]：股票按（市场，代码）查询领域规则（市场推断 / 矛盾 400 / 币种推导，issue #693）

mod common;
mod fund_add;
mod fund_trade;
mod holdings_as_of;
mod instrument_create;
mod instrument_delete;
mod instrument_list;
mod instrument_manual_create;
mod manual_price;
mod pnl;
mod predicates;
mod stock_lookup;
mod trade;
mod trend;

// instrument_list 子模块的断言沿用原调用路径 `super::crud::…`：在此把投资域
// crud 模块引入 tests 命名空间，供子模块以原路径解析（纯移动保形）。
use super::crud;
