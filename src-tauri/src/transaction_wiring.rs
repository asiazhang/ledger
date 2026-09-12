//! 交易域接缝的组合安装入口（spec #1086 / issue #1180）：核心交易域对投资/商户/
//! 币种/物品/保单/账户六向的注册点（ADR-0112 决策 5 挂载点⑤，#1092 落地）在七个
//! 建库/启动入口逐字重复同一组 provider 级 `install_*` 调用——第 8 条接缝出现时要改
//! 7 个文件，新入口也容易漏装某一向。
//!
//! 本模块把「六向装齐」收敛为一处：[`install_all`] 逐 provider 调用，入口侧只留
//! 一行。形态仍是「下层定义注册点、上层注册实现、壳层启动时接线」——聚合的只是壳层
//! 接线动作，注册点契约与实现住址不变，provider 级 `install_*` 保持独立可测。
//!
//! 未注册即码化错误（失败可见，不静默丢副作用），故无需 `BOOT_WIRING` 扫描兜底
//! （#1092 判据不变）：任一入口漏调 [`install_all`] 都在该入口触达的写/读路径上以
//! 既有 `transaction.*-unregistered` 错误码红给测试。

use crate::accounts;
use crate::currencies;
use crate::investment;
use crate::item;
use crate::merchants;
use crate::policy;

/// 一次性装入六个提供域的挂载点实现：投资 6 钩子（计划装配/副作用/回退/释放 +
/// 标的反查 + 转换两腿）、商户名钩子组、本位币基准、来源列保单/物品反查与出资账户
/// 视图。
///
/// 幂等：各注册点进程级一次、重复注册保留首次，多入口重复调用安全。调用须先于任何
/// 建库/写库（与逐行 provider 级调用同序）。
pub fn install_all() {
    investment::install_transaction_hooks();
    merchants::install_merchant_hooks();
    currencies::install_base_currency_hook();
    item::install_source_hook();
    policy::install_source_hook();
    accounts::install_funding_account_hook();
}
