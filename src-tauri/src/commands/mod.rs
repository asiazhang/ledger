//! IPC 命令壳枢纽：命令域模块声明 + 扁平 `pub use` 链（注册路径 `commands::<name>`，
//! ADR-0047）。写命令的「命令 → 写操作身份」声明已随 ADR-0073（spec #523）内化
//! 进统一写入口 `write_entry` 的调用点，壳侧接线由源码扫描守门核对
//! （`signals_cross_check`，ADR-0073 决策 5）。

pub mod accounts;
pub mod ai;
pub mod backup;
pub mod boot;
pub mod budget;
pub mod categories;
pub mod currencies;
pub mod dashboard;
pub mod data_location;
pub mod encryption;
pub mod financial_freedom;
pub mod insurer;
pub mod investment;
pub mod item;
pub mod logs;
pub mod merchants;
pub mod physical_asset;
pub mod policy;
pub mod reports;
pub mod scheduled;
pub mod search;
pub mod settings;
pub mod sync;
pub mod transactions;

pub use accounts::*;
pub use ai::*;
pub use backup::*;
pub use boot::*;
pub use budget::*;
pub use categories::*;
pub use currencies::*;
pub use dashboard::*;
pub use data_location::*;
pub use encryption::*;
pub use financial_freedom::*;
pub use insurer::*;
pub use investment::*;
pub use item::*;
pub use logs::*;
pub use merchants::*;
pub use physical_asset::*;
pub use policy::*;
pub use reports::*;
pub use scheduled::*;
pub use search::*;
pub use settings::*;
pub use sync::*;
pub use transactions::*;
