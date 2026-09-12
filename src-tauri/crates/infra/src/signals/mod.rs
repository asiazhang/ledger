//! 失效信号映射单点（ADR-0044，spec #311 / issue #330）：「写操作 → 失效信号」的
//! 唯一判定知识。
//!
//! 职责分界（ADR-0044 决策 7）：本模块承载**知识**（谁发什么）——强类型写操作身份
//! [`WriteOp`]、结果证据 [`WriteEvidence`]、信号 [`Signal`] 与纯函数 [`signals_for`]；
//! **机制**（怎么发）留在 `events.rs`（事件名常量 + 发射器接缝 `SignalEmitter` +
//! `EVENT_APP` 镜像句柄 + 主线程非阻塞投递，spec #364/#366），本模块经 [`emit_all`] /
//! [`emit_for`] 遍历信号集把事件名投递给发射器。
//!
//! #1129 起升为目录模块（ADR-0111 决策 2 / 决策 3），本文件只做声明与再导出：
//! - [`write_op`]：写操作身份闭集与单一来源宏 `write_op_set!`（ADR-0102，宏定义
//!   与调用就地）；
//! - [`evidence`]：结果证据 [`WriteEvidence`]；
//! - [`mapping`]：信号 [`Signal`] 与唯一映射 [`signals_for`]（ADR-0044 决策 1）；
//! - [`emit`]：发射助手 [`emit_all`] / [`emit_for`]；
//! - 测试外挂 `tests/`（`tests.rs` 声明 + `tests/mapping.rs` / `tests/emit_blocking.rs`），
//!   与 `db/tests/` 同形；既有调用点路径 `crate::signals::…` 经再导出零改动。
//!
//! - **键是强类型写操作身份**，不沿用命令名字符串（ADR-0044 决策 2）：HTTP handler
//!   无命令名（axum），且两壳命令面不对称，字符串键必然漂移；IPC 命令与 HTTP 端点
//!   写同一数据时共享同一 [`WriteOp`]（如账户删除命令与 `DELETE /api/v1/accounts/{id}`）。
//! - **映射闭集穷举**（ADR-0044 决策 3）：[`signals_for`] 对 [`WriteOp`] 穷尽 `match`
//!   （编译期防「enum 新增变体漏改映射」），「零信号」是显式登记行而非缺行——
//!   「不发」是决策（附动机注释），不是遗漏。
//! - **条件信号三类归一化**（ADR-0044 决策 4）：价格实际写入 / 黑洞即建 / 商户即建
//!   统一到 [`WriteEvidence`] 形状，映射表只保留一份「实际写入」判定
//!   （见 [`WriteEvidence::price_written`]），调用方把各自域内结果归一化为证据即可。
//! - **自动备份深路径**（ADR-0044 决策 5）：无命令身份、经 `events::EVENT_APP` 镜像
//!   句柄发射，登记为映射表特例条目 [`WriteOp::AutoBackupDeepPath`]，不做命令键——
//!   三个 `ledger:*` 信号的生产者清单由此单点可查。
//!
//! 旧机制已随 #335 收缩删除：`events::REFERENCE_WRITE_COMMANDS` /
//! `is_reference_write` / `emit_reference_changed` 不再存在，「谁发什么」的
//! 判定知识唯一载体是本模块。壳侧接线由源码扫描守门测试（`signals_cross_check`，
//! ADR-0073 决策 5）兜底：从两壳 `write_entry` 调用点扫描提取「声明壳, 身份」
//! 派生表（例外白名单登记不经入口的声明写命令）+ 反向守门（`db::write`/发射
//! 调用必经写入口）——「新写命令忘了声明身份」「绕开入口写库」均在测试期即红。
//! 手写声明表（IPC/HTTP 两张，约 190 行）已随 ADR-0073 消亡为扫描派生物。
//!
//! 写操作边界：本闭集收录「以写为意图」的操作（DB 行写入、KV / 指针文件写入、
//! 备份产物、进程级设置镜像推送）；纯读命令与控制类命令（`restart_app` /
//! `open_log_dir`）不是写操作，不入集。

pub mod emit;
pub mod evidence;
pub mod mapping;
pub mod write_op;

pub use emit::{emit_all, emit_for};
pub use evidence::WriteEvidence;
pub use mapping::{Signal, signals_for};
pub use write_op::WriteOp;

#[cfg(test)]
mod tests;
