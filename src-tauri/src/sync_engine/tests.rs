//! 多端同步域测试索引（issue #855 / #856 / #857 / #858 / #859）。
//!
//! - [`common`]：域薄皮（双端建库、交易语义输入构造器、业务字段行读取、内存假 Transport、
//!   本地 WebDAV 桩）
//! - [`device`]：DeviceId 首用生成与持久化、换库新标识
//! - [`total_order`]：跨端全序 (clock, device_id) 确定性
//! - [`engine`]：幂等重放与「A 端写 → B 端重放后账本状态一致」闭环
//! - [`merge`]：双端合并语义——LWW、OccurrenceKey 防双扣、ParkedOp 挂起（issue #856）
//! - [`ledger_setting`]：账本级设置（本位币基准）——并发 LWW 合并与同步后折算确定性（issue #858）
//! - [`checkpoint`]：Checkpoint 快照、新端引导、位点与截断机制（issue #857）
//! - [`wire`]：op 信封序列化往返（wire 形态稳定性）
//! - [`envelope`]：SyncEnvelope 信封加密往返与码化错误（issue #859）
//! - [`transport`]：WebDAV 哑字节通道后端对本地桩的行为与错误归一（issue #859）
//! - [`channel`]：通道目录布局/manifest/同步轮次与两端文件交换集成（issue #859）

mod channel;
mod checkpoint;
mod common;
mod device;
mod engine;
mod envelope;
mod ledger_setting;
mod merge;
mod parked;
mod total_order;
mod transport;
mod wire;
