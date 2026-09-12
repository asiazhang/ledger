//! `signals` 模块的单元测试，按行为主题拆为子模块（#1129，纯移动），与
//! `db/tests.rs` 同形——测试外挂于生产模块，不参与结构守门（ADR-0056 决策 5）：
//! - `mapping`：映射知识层（谁发什么）——逐写操作身份直测 [`signals_for`] 的
//!   返回值形状，含零信号显式断言（ADR-0044 决策 3）；
//! - `emit_blocking`：发射机制层（怎么发）——闸门式假发射器钉死「发射器阻塞期间
//!   写路径仍及时返回、放行后信号最终到达」（spec #366 / ADR-0054）。
//!
//! [`signals_for`]: crate::signals::signals_for

mod emit_blocking;
mod mapping;
