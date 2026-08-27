//! 物品（Item）领域模块（spec #113）。
//!
//! 接缝：
//! - [`cost`]（DailyUsageCost 权威）：「每天使用成本」纯计算，分子（总成本 − 残值）
//!   ÷ 分母（购买日 → 目标日的日历天数，含起止两端）。
//!
//! 依赖方向恒为「命令层 → item」：本模块不反向依赖命令层。

pub mod cost;

#[cfg(test)]
mod tests;
