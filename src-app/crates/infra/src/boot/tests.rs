//! `boot` 模块的单元测试，按行为主题拆为子模块（#1131 自 `db/tests/` 随模块
//! 同迁；四个原挂生产模块旁的 `tests.rs` 与 `passphrase_cache` 内联测试随后
//! 统一外挂入本目录，与 `db/tests/`、`signals/tests/` 同形）：
//! - `book_registry`：账本注册表内核——双格式解析、损坏判定与登记变更不变量
//!   （issue #832 / #833 / #836）；
//! - `data_location`：DataLocation 引导解析（注册表双格式、损坏回退、文件保全）
//!   与更改意图读写（ADR-0018 / ADR-0089）；
//! - `disposition`：启动期库文件处置判定的三态分派与启动失败门（issue #601）；
//! - `encryption`：SQLCipher 引擎基座（issue #569 / ADR-0075）——依赖切换
//!   不变量（未设密钥保持明文）、建连密钥缝、文件头探测三态、整库转换与
//!   解锁、忘记口令重置守卫；
//! - `passphrase_cache`：本机记忆主口令的形态判别与缓存条目分域
//!   （issue #662 / #836）。

mod book_registry;
mod data_location;
mod disposition;
mod encryption;
mod passphrase_cache;
