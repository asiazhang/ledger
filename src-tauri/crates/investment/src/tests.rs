//! 投资域测试索引（issue #257：原单文件 tests.rs 按行为主题拆分为目录子模块，
//! 纯移动——断言、夹具与 mock 语义不变，测试不增删不改名；#401 域归位随迁）。
//!
//! - [`common`]：域特有标的种子与交易输入构造等共享脚手架（建库与跨域夹具已上收
//!   统一工厂 `tauri_app_lib::test_support`，spec #728 / 票 #755）
//! - [`instrument_list`]：标的列表、搜索、invested 派生、CRUD 与持仓视图
//! - [`instrument_create`]：标的创建的来源标记（同步 / 手动）与复用不覆盖
//! - [`instrument_manual_create`]：手动创建入口守卫（类型白名单 + 名称必填，issue #290）
//! - [`instrument_delete`]：自建标的删除守卫两态（issue #292 / ADR-0036 决策 5）
//! - [`fund_add`]：按代码即拉添加基金（注入 stub，issue #301 / ADR-0038）
//! - [`manual_price`]：手动报价两落点与信号发射判定（issue #291 / ADR-0036）
//! - [`predicates`]：「持仓标的」判定谓词 ↔ v_holdings 视图一致性绑定
//! - [`price_channel`]：价格通道派生判定与读路径接线（issue #1060）
//! - [`trade`]：buy/sell 写入与买卖明细查询（命名对齐源码 trade 模块）
//! - [`convert`]：基金转换（convert）写入——转出腿 FIFO 消耗与结转成本、转入批次
//!   建仓与闭合、零已实现盈亏、余额不变、两腿时点持仓、守卫与回退/删除（ADR-0099）
//! - [`fund_trade`]：场外基金申赎记账——金额权威、单价反算、成本锚定与盈亏闭合不变式（issue #302 / ADR-0038）
//! - [`pnl`]：已实现盈亏汇总
//! - [`read_snapshot`]：多语句读闭包的快照一致性探针——总量=分量和与分子分母
//!   同时点（issue #1699）
//! - [`cumulative_pnl`]：累计收益按币种聚合（未实现 + 已实现 + 累计分红三腿相加，issue #1077）
//! - [`holdings_summary`]：持仓合计按币种分组读投影与可投资资产分子提取
//!   （issue #1196 / ADR-0114 跨账本汇总的域读接缝）
//! - [`holdings_native`]：持仓逐行本位币列（软形态）与累计收益折本位币单值
//!   （硬形态）——统一单币种显示的读投影（issue #1797）
//! - [`mwr`]：资金加权收益率（ADR-0115 / issue #1195）——XIRR 求解器手算样本对齐
//!   与无解不给数、读投影场景矩阵（单笔/定投/部分卖出/分红/转换两腿/缺价跳过/
//!   币种分组/DRIP 自相抵/区间期初市值）
//! - [`overview`]：投资概览读数（spec #1532 / issue #1536）——可投资资产两腿拆分
//!   与既有单点恒等、两腿折本位币（负向：删折算即红）、缺汇率码化上抛、隐藏账户
//!   与缺价持仓排除、无投资账户的零值与引导事实
//! - [`dividend`]：现金分红（dividend）写入——现金腿 + 标的扩展行、任意在用账户、
//!   无持仓可录、守卫齐全、kind 变更拒绝与改 / 删回退（issue #1078 / ADR-0109）
//! - [`trend`]：走势查询（单标的 / 组合）
//! - [`holdings_as_of`]：时点持仓推算
//! - [`instrument_type_string`]：`InstrumentType` 字符串面（宏同体派生，ADR-0108）
//! - [`ledger_tab`]：投资明细列表读命令（ADR-0135 / issue #1778）——四维过滤逐维
//!   断言（账户涉及语义含出资/到账端、标的含 convert 两腿命中、kind 子集、日期
//!   区间）、排序与 offset 分页 + total、软删排除、各 kind 投影字段逐项断言、
//!   页与总数的读快照探针（#1699 / #1702）
//! - [`instrument_type_check`]：`instrument_type` CHECK 字面量 ↔ `ALL` 互核（ADR-0108）
//! - [`market_check`]：`market` CHECK 字面量 ↔ `Market::ALL` 互核（issue #1673，ADR-0108 同款）
//! - [`stock_lookup`]：股票按（市场，代码）查询领域规则（市场推断 / 矛盾 400 / 币种推导，issue #693）
//! - [`stock_create`]：股票创建增强的行情往返路由与落库接缝（权威名称 + 现价 / 降级市场保留，issue #694）
//! - [`stock_add`]：「添加投资标的」股票侧录入——通道解析、查询遍历与识别落库（issue #697）
//! - [`staleness`]：价格过期检查（issue #1190）——本地水位检查的判定矩阵
//!   （水位阈值边界 / 北京日历日换算 / 持仓缺现价 / 手动与无来源通道豁免）
//! - [`split`]：份额调整（split）写入与改删——按比例重述与尾差归末批次、部分卖出按
//!   重述后每份成本结算（决策 2 唯一钉死处）、逐批次 before 快照精确回补（改 / 删）、
//!   下游在用消耗守卫、无 split 行消耗回算恒等绑定、kind 变更拒绝
//!   （ADR-0106 / issue #1049 + #1050 + #1051）

mod common;
mod constant_price;
mod convert;
mod cumulative_pnl;
mod dividend;
mod fund_add;
mod fund_trade;
mod holdings_as_of;
mod holdings_native;
mod holdings_summary;
mod instrument_create;
mod instrument_delete;
mod instrument_list;
mod instrument_manual_create;
mod instrument_type_check;
mod instrument_type_string;
mod ledger_tab;
mod manual_price;
mod market_check;
mod mwr;
mod overview;
mod pnl;
mod predicates;
mod price_channel;
mod read_snapshot;
mod split;
mod staleness;
mod stock_add;
mod stock_create;
mod stock_lookup;
mod trade;
mod trend;

// instrument_list 子模块的断言沿用原调用路径 `super::crud::…`：在此把投资域
// crud 模块引入 tests 命名空间，供子模块以原路径解析（纯移动保形）。
use super::crud;
