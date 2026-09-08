//! 多端同步域测试索引（issue #855）。
//!
//! - [`common`]：域薄皮（双端建库、交易语义输入构造器、业务字段行读取）
//! - [`device`]：DeviceId 首用生成与持久化、换库新标识
//! - [`total_order`]：跨端全序 (clock, device_id) 确定性
//! - [`engine`]：幂等重放与「A 端写 → B 端重放后账本状态一致」闭环
//! - [`wire`]：op 信封序列化往返（wire 形态稳定性）

mod common;
mod device;
mod engine;
mod total_order;
mod wire;
