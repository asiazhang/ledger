//! 行情同步测试（issue #89 外迁）：HTTP 重试/多主机/解析、价格换算、持久化与编排行为。
//! HTTP 层通过本地 HTTP 服务独立测试，不依赖真实网络。
//!
//! #256 按行为主题拆为子模块（纯移动）：
//! - `http_client`：HTTP 重试与多主机切换；
//! - `instrument_sync`：全量同步与 clist 报文解析；
//! - `holding_price_sync`：持仓价格增量同步与 ulist / 日 K 报文解析；
//! - `run_sync`：分页编排、取消、锁、重入守卫与价格失效信号判定。

mod common;

mod holding_price_sync;
mod http_client;
mod instrument_sync;
mod run_sync;
