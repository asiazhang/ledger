//! 行情同步测试（issue #89 外迁）：HTTP 重试/多主机/解析、价格换算与增量同步行为。
//! HTTP 层通过本地 HTTP 服务独立测试，不依赖真实网络。
//!
//! #256 按行为主题拆为子模块（纯移动）：
//! - `http_client`：HTTP 重试与多主机切换；
//! - `holding_price_sync`：持仓价格增量同步与 ulist / 日 K 报文解析；
//! - `fund_search`：东财基金搜索报文解析与命中挑选（issue #301，fixture 驱动）；
//! - `fund_nav`：历史净值报文解析、水位窗口与 Referer 传播（issue #303，fixture 驱动）；
//! - `stock_quote`：股票单点行情报文解析、类型特征探测与命中挑选（issue #693，fixture 驱动）。
//!
//! 全量同步（clist 报文解析、分页编排、取消与重入守卫）已随 ADR-0081 决策 3
//! 退役删除（issue #698）。

mod fund_nav;
mod fund_search;
mod holding_price_sync;
mod http_client;
mod stock_quote;
