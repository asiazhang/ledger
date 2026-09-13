//! 多端同步域测试索引（issue #855 / #856 / #857 / #858 / #859 / #860）。
//!
//! - [`common`]：域薄皮（双端建库、交易语义输入构造器、业务字段行读取、内存假 Transport、
//!   本地 WebDAV 桩）
//! - [`device`]：DeviceId 首用生成与持久化、换库新标识（被测对象自 #1089 起
//!   住协议 crate `ledger-sync-protocol::device`）
//! - [`total_order`]：跨端全序 (clock, device_id) 确定性
//! - [`engine`]：幂等重放与「A 端写 → B 端重放后账本状态一致」闭环
//! - [`merge`]：双端合并语义——LWW、OccurrenceKey 防双扣、ParkedOp 挂起（issue #856）
//! - [`ledger_setting`]：账本级设置（本位币基准）——并发 LWW 合并与同步后折算确定性（issue #858）
//! - [`reference_data`]：参考数据字典（账户/分类/商户）全域 op 产出与收敛（issue #860）
//! - [`budget`]：预算全域 op 产出、唯一冲突挂起（issue #860）
//! - [`insurance`]：保司字典 + 保单全域 op 产出、依赖缺失挂起重投递自愈（issue #860）
//! - [`investment`]：投资域与 AI 导入路径全域 op 产出——buy/sell 三件套重放、标的字典、
//!   汇率/现价/手动报价与 IdempotencyKey 去重独立性（issue #861）
//! - [`convert`]：基金转换的语义命令重放——create/update/delete 三 op 收敛、
//!   结转成本随命令携带与本地 FIFO 重建、schema 超前挂起与旧载荷兼容（issue #980）
//! - [`split`]：份额调整的语义命令重放——create/update/delete 三 op 收敛、批次
//!   重述本地重建与最终持仓 / 批次总成本比对、依赖倒挂与旧载荷挂起（issue #1053）
//! - [`dividend`]：现金分红的语义命令重放——create/update/delete 三 op 收敛、
//!   扩展行自包含（无 FIFO 重建）、标的依赖倒挂挂起自愈、币种分叉码化挂起（issue #1078）
//! - [`item`]：物品全域 op 产出、源端折算随行（issue #860）
//! - [`physical_asset`]：实物资产全域 op 产出、并发估值全部存活（issue #860）
//! - [`scheduled_plan`]：定时计划全域 op 产出与收敛（issue #860）
//! - [`transaction_funding`]：出资账户的重放收敛与旧格式 op 前向兼容（issue #939）
//! - [`checkpoint`]：Checkpoint 快照、新端引导、位点与截断机制（issue #857）
//! - [`wire`]：op 信封序列化往返（wire 形态稳定性）
//! - [`envelope`]：SyncEnvelope 信封加密往返与码化错误（issue #859）
//! - [`transport`]：WebDAV 哑字节通道后端对本地桩的行为与错误归一（issue #859）
//! - [`transport_s3`]：S3 兼容对象存储后端对本地桩的行为、寻址、分片与错误归一（issue #1216）
//! - [`probe`]：保存前「测试连接」的读取探针——缺对象=连通、权限/目标/网络分层（issue #1219）
//! - [`channel`]：通道目录布局/manifest/同步轮次与两端文件交换集成（issue #859）

mod budget;
mod channel;
mod checkpoint;
pub(crate) mod common;
mod convert;
mod device;
mod dividend;
mod engine;
mod envelope;
mod insurance;
mod investment;
mod item;
mod ledger_setting;
mod merge;
mod parked;
mod physical_asset;
mod probe;
mod reference_data;
mod scheduled_plan;
mod split;
mod total_order;
mod transaction_funding;
mod transport;
mod transport_s3;
mod wire;
